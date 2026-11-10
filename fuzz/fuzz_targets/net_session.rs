#![no_main]

use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_net::{Request, Session, SessionConfig, MAX_OUTGOING_BYTES};

fuzz_target!(|data: &[u8]| {
    // The first byte is the size of each read; the rest is the stream.
    let Some((&size, stream)) = data.split_first() else {
        return;
    };
    let config = SessionConfig {
        network: Network::Regtest,
        nonce: 0x0102_0304_0506_0708,
        tip_height: 100,
    };
    let mut session = Session::new(config, 1_000);
    let step = usize::from(size) * 17 + 1;
    let mut now = 1_000u64;
    for chunk in stream.chunks(step) {
        now += 1;
        // It must never panic. A drop names a rule and ends the session.
        match session.receive(chunk, now) {
            Ok(_) => {
                let _ = session.send(Request::GetAddr);
            }
            Err(drop) => {
                assert!(drop.to_string().starts_with(drop.rule()));
                assert_eq!(session.tick(now), Err(drop));
                return;
            }
        }
        // What waits to be sent never passes the cap.
        assert!(session.take_outgoing().len() <= MAX_OUTGOING_BYTES);
    }
});
