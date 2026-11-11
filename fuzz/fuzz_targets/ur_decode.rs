#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_ur::{Decoder, Encoder};

fuzz_target!(|data: &[u8]| {
    // Any text, one decoder that keeps going across the lines of the input.
    let text = String::from_utf8_lossy(data);
    let mut d = Decoder::new();
    for line in text.split('\n') {
        match d.receive(line) {
            Ok(p) => {
                assert!(p.estimated_fraction <= 1.0);
                if p.complete {
                    assert!(d.result().is_some());
                }
            }
            Err(e) => assert!(e.to_string().starts_with("UR-")),
        }
    }
    let _ = d.result();

    // A real sequence, with the input's bytes as the message: every part
    // of it decodes, in order, to the same bytes.
    let Some((&max, message)) = data.split_first() else {
        return;
    };
    let payload: Vec<u8> = message.iter().copied().take(4000).collect();
    let max = 10 + usize::from(max) * 7;
    let Ok(mut e) = Encoder::new("bytes", &payload, max, 10) else {
        return;
    };
    let mut d = Decoder::new();
    for _ in 0..e.seq_len().min(2000) {
        let _ = d.receive(&e.next_part());
    }
    if let Some((ty, got)) = d.result() {
        assert_eq!(ty, "bytes");
        assert_eq!(got, payload);
    }
});
