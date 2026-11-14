#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_link::numbers::{Inbox, Outbox, MAGIC_INBOX, MAGIC_OUTBOX};
use openbmt_link::shopln::{
    LightningReport, StoreMessage, StoredMessage, MAGIC_LIGHTNING_REPORT, MAGIC_STORE, MAGIC_STORED,
};

/// Every decoder of the number books and of the Shop Lightning messages:
/// what reads writes back to exactly the input.
fn check(data: &[u8]) {
    if let Ok(v) = Inbox::from_bytes(data) {
        assert_eq!(v.to_bytes(), data);
    }
    if let Ok(v) = Outbox::from_bytes(data) {
        assert_eq!(v.to_bytes(), data);
    }
    if let Ok(v) = StoreMessage::from_bytes(data) {
        assert_eq!(v.to_bytes(), data);
    }
    if let Ok(v) = StoredMessage::from_bytes(data) {
        assert_eq!(v.to_bytes(), data);
    }
    if let Ok(v) = LightningReport::from_bytes(data) {
        assert_eq!(v.to_bytes(), data);
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind each frame header (magic and version 1), so that
    // the input gets past the magic to the parts.
    for magic in [
        MAGIC_INBOX,
        MAGIC_OUTBOX,
        MAGIC_STORE,
        MAGIC_STORED,
        MAGIC_LIGHTNING_REPORT,
    ] {
        let mut framed = magic.to_vec();
        framed.push(1);
        framed.extend_from_slice(data);
        check(&framed);
    }
});
