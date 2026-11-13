#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_counter_ffi::{tag_content, tap_answer};

// The NFC tag's command parser (TP-14): `tap_answer` with an arbitrary
// session blob, with the content a sale gives or none, and with arbitrary
// command bytes, and then as a whole tap, the commands one after the other
// on the session each one returns. Whatever the input, the answer ends with
// a status word and holds at most 255 bytes before it, the session it
// returns is at most the file and its length, no sale answers 6A82 and
// drops the session, and a command longer than 261 bytes is refused with
// 6700 and changes nothing.
fuzz_target!(|data: &[u8]| {
    let Some((&shape, rest)) = data.split_first() else {
        return;
    };
    // The content: none, a file the library makes from text, or raw bytes.
    let text =
        String::from_utf8_lossy(rest.get(..rest.len().min(300)).unwrap_or(&[])).to_lowercase();
    let made = tag_content(&text);
    let raw = rest.get(..rest.len().min(2100)).unwrap_or(&[]);
    let content: Option<&[u8]> = match shape & 3 {
        0 => None,
        1 | 2 => made.as_deref(),
        _ => Some(raw),
    };
    // One command on a session that is the input itself.
    let (session, response) = tap_answer(rest, content, rest);
    check(content, &session, &response, rest);
    if rest.len() > 261 && valid(content) {
        assert_eq!(response, vec![0x67, 0x00]);
    }
    // A tap: the input cut into commands of the lengths its bytes say.
    let mut session = Vec::new();
    let mut at = 0usize;
    while let Some(&n) = rest.get(at) {
        let end = (at + 1 + usize::from(n)).min(rest.len());
        let apdu = rest.get(at + 1..end).unwrap_or(&[]);
        let (next, response) = tap_answer(&session, content, apdu);
        check(content, &next, &response, apdu);
        if apdu.len() > 261 && valid(content) {
            assert_eq!(next, session);
        }
        session = next;
        at = end;
    }
});

fn check(content: Option<&[u8]>, session: &[u8], response: &[u8], apdu: &[u8]) {
    assert!(response.len() >= 2 && response.len() <= 255 + 2);
    assert!(session.len() <= 3 + 2050);
    if !valid(content) {
        assert_eq!(response, vec![0x6A, 0x82], "{apdu:02x?}");
        assert!(session.is_empty());
    }
}

/// Whether the content is a file the tag serves: at least the length field
/// and at most 2050 bytes.
fn valid(content: Option<&[u8]>) -> bool {
    content.is_some_and(|c| (2..=2050).contains(&c.len()))
}
