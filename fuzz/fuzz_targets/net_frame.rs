#![no_main]

use bitcoin::p2p::Magic;
use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_net::FrameReader;

fuzz_target!(|data: &[u8]| {
    // The first byte says whether the handshake is over, the second the
    // size of each read; the rest is the stream.
    let Some((&flags, rest)) = data.split_first() else {
        return;
    };
    let Some((&size, stream)) = rest.split_first() else {
        return;
    };
    let mut reader = FrameReader::new(Magic::from(Network::Regtest));
    if flags & 1 == 1 {
        reader.set_handshake_done();
    }
    let step = usize::from(size) * 257 + 1;
    for chunk in stream.chunks(step) {
        if reader.push(chunk).is_err() {
            return;
        }
        // It must never panic, and it never holds more than the cap.
        loop {
            match reader.next() {
                Ok(Some(frame)) => {
                    assert!(frame.payload.len() <= openbmt_net::MAX_BLOCK_PAYLOAD as usize);
                    // A parse may refuse; it must not panic.
                    let _ = frame.parse();
                }
                Ok(None) => break,
                Err(drop) => {
                    assert!(drop.to_string().starts_with(drop.rule()));
                    return;
                }
            }
        }
        assert!(reader.buffered() <= reader.buffer_cap());
    }
});
