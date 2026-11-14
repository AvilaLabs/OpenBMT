#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_owner::web_order::{NoticeAction, OrdersReply, PairingCode, RangeReply, StoreOrder};

// The first byte picks the parser, the rest is what it reads: the list of
// orders, the reply to a request for orders, the replies to a range and to a
// notice, and the bytes of a pairing code. None may panic; a pairing code
// that reads must write back to exactly the input.
fuzz_target!(|data: &[u8]| {
    let Some((&pick, rest)) = data.split_first() else {
        return;
    };
    match pick % 5 {
        0 => {
            let _ = StoreOrder::parse_list(rest);
        }
        1 => {
            // A reply that reads holds the list that reads alone.
            if let Ok(reply) = OrdersReply::parse(rest) {
                assert_eq!(StoreOrder::parse_list(rest), Ok(reply.orders));
            }
        }
        2 => {
            let _ = RangeReply::parse(rest);
        }
        3 => {
            let _ = NoticeAction::parse(rest);
        }
        _ => {
            if let Ok(code) = PairingCode::from_bytes(rest) {
                assert_eq!(code.to_bytes(), rest);
            }
        }
    }
});
