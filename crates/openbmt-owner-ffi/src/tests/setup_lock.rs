//! The lock on the setup text, through `dispatch` as the owner app calls it
//! (`docs/OWNER-APP-RULES.md` OA-60 to OA-62, `docs/LOCK-RULES.md`): ops 100
//! to 103.

use openbmt_lock::{decode, HEADER_LEN, MAGIC_LOCKED};

use super::wire::{frame, Rd, W};
use crate::{dispatch, Registry};

const TEXT: &[u8] = b"owner_secret=00112233\nkey1_account=44556677\n";
const PIN: &str = "482913";

fn call(op: u16, fields: &[u8]) -> Vec<u8> {
    dispatch(&Registry::new(), &frame(op, 0, fields))
}

fn lock(pin: &str, text: &[u8]) -> Vec<u8> {
    let response = call(101, &W::new().string(pin).bytes(text).0);
    let mut r = Rd::ok(&response);
    let out = r.bytes();
    r.done();
    out
}

fn unlock(locked: &[u8], pin: &str) -> Vec<u8> {
    let response = call(102, &W::new().bytes(locked).string(pin).0);
    let mut r = Rd::ok(&response);
    let out = r.bytes();
    r.done();
    out
}

fn change(locked: &[u8], current: &str, new: &str) -> Vec<u8> {
    let response = call(103, &W::new().bytes(locked).string(current).string(new).0);
    let mut r = Rd::ok(&response);
    let out = r.bytes();
    r.done();
    out
}

fn form(bytes: &[u8]) -> bool {
    let response = call(100, &W::new().bytes(bytes).0);
    let mut r = Rd::ok(&response);
    let out = r.bool();
    r.done();
    out
}

/// The refusal of a call: status, rule and the words' heading.
fn refusal(response: &[u8]) -> (u8, String, String) {
    let fault = Rd::error(response).expect("refused");
    (fault.status, fault.rule, fault.words.heading)
}

#[test]
fn oa_60_the_setup_text_is_locked_opened_and_changed_through_dispatch() {
    assert!(!form(TEXT) && !form(b"") && !form(b"OBM7"));
    let locked = lock(PIN, TEXT);
    assert_eq!(&locked[..4], &MAGIC_LOCKED);
    assert!(form(&locked));
    // A frame of the shared crate, which the text is not in.
    let parsed = decode(&locked).expect("a frame");
    assert_eq!(locked.len(), HEADER_LEN + TEXT.len() + 16);
    assert_eq!(parsed.header.body_len as usize, TEXT.len() + 16);
    assert!(!locked.windows(TEXT.len()).any(|w| w == TEXT));
    assert_eq!(unlock(&locked, PIN), TEXT);
    // Every lock is made with a new salt and a new nonce.
    let again = lock(PIN, TEXT);
    let other = decode(&again).expect("a frame");
    assert_ne!(parsed.header.salt, other.header.salt);
    assert_ne!(parsed.header.nonce, other.header.nonce);
    assert_eq!(unlock(&again, PIN), TEXT);
    // The PIN changes in one call, under a new salt and nonce.
    let moved = change(&locked, PIN, "a longer passphrase");
    let moved_header = decode(&moved).expect("a frame").header;
    assert_ne!(moved_header.salt, parsed.header.salt);
    assert_ne!(moved_header.nonce, parsed.header.nonce);
    assert_eq!(unlock(&moved, "a longer passphrase"), TEXT);
    assert!(Rd::error(&call(102, &W::new().bytes(&moved).string(PIN).0)).is_some());
    // The old frame is as it was: the library stores nothing and the app
    // swaps the file only when the call is Ok.
    assert_eq!(unlock(&locked, PIN), TEXT);
}

#[test]
fn oa_60_the_library_stores_nothing_of_the_lock() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("src");
    let text = std::fs::read_to_string(dir.join("setup_lock.rs")).expect("readable");
    let code = text.split("\n#[cfg(test)]").next().expect("the code");
    // The only file it touches is the random source, and only to read.
    assert_eq!(code.matches("std::fs::").count(), 1);
    assert!(code.contains("std::fs::File::open(path)"));
    for needle in ["write(", "create(", "OpenOptions", "remove_file", "rename("] {
        assert!(!code.contains(needle), "{needle}");
    }
}

#[test]
fn oa_61_a_pin_the_rules_do_not_take_locks_nothing() {
    for (pin, rule) in [
        ("12345", "pin-weak"),
        ("abcdefg", "pin-weak"),
        ("", "pin-weak"),
        (&"9".repeat(129), "pin-long"),
    ] {
        let response = call(101, &W::new().string(pin).bytes(TEXT).0);
        let (status, got, heading) = refusal(&response);
        assert_eq!((status, got.as_str()), (1, rule), "{pin:?}");
        assert!(heading.starts_with("PIN too"), "{heading}");
    }
    assert!(Rd::error(&call(101, &W::new().string("123456").bytes(TEXT).0)).is_none());
    assert!(Rd::error(&call(101, &W::new().string("a passphrase").bytes(b"").0)).is_none());
    // A change to a PIN that is not taken changes nothing, and the new PIN
    // is judged before the old one is tried.
    let locked = lock(PIN, TEXT);
    let response = call(103, &W::new().bytes(&locked).string(PIN).string("123").0);
    assert_eq!(refusal(&response).1, "pin-weak");
    let response = call(
        103,
        &W::new().bytes(&locked).string("000000").string("123").0,
    );
    assert_eq!(refusal(&response).1, "pin-weak");
    assert_eq!(unlock(&locked, PIN), TEXT);
}

#[test]
fn oa_62_a_wrong_pin_is_refused_and_told_from_a_frame_that_is_not_readable() {
    let locked = lock(PIN, TEXT);
    let response = call(102, &W::new().bytes(&locked).string("482914").0);
    let (status, rule, heading) = refusal(&response);
    assert_eq!(
        (status, rule.as_str(), heading.as_str()),
        (1, "pin-wrong", "Wrong PIN")
    );
    let response = call(
        103,
        &W::new().bytes(&locked).string("482914").string("654321").0,
    );
    assert_eq!(refusal(&response).1, "pin-wrong");
    // A changed ciphertext gives the same answer as a wrong PIN.
    let mut flipped = locked.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 1;
    let response = call(102, &W::new().bytes(&flipped).string(PIN).0);
    assert_eq!(refusal(&response).1, "pin-wrong");
    // A cut frame, plain text and an empty file are not readable with any PIN.
    for bad in [
        &locked[..locked.len() - 1],
        TEXT,
        b"",
        &locked[..HEADER_LEN],
    ] {
        let response = call(102, &W::new().bytes(bad).string(PIN).0);
        let (status, rule, heading) = refusal(&response);
        assert_eq!((status, rule.as_str()), (1, "OBMx"), "{} bytes", bad.len());
        assert_eq!(heading, "Locked setup not readable");
        let response = call(103, &W::new().bytes(bad).string(PIN).string("654321").0);
        assert_eq!(refusal(&response).1, "OBMx");
    }
}

#[test]
fn oa_62_the_ops_read_their_request_completely_and_hold_no_node() {
    let locked = lock(PIN, TEXT);
    let good = W::new().bytes(&locked).string(PIN).string("654321").0;
    assert!(Rd::error(&call(103, &good)).is_none());
    // Every cut and a byte too many are refused.
    for n in 0..good.len() {
        assert!(Rd::error(&call(103, &good[..n])).is_some(), "cut at {n}");
    }
    let mut longer = good.clone();
    longer.push(0);
    assert!(Rd::error(&call(103, &longer)).is_some());
    // A PIN that is not UTF-8 is not a string.
    let bad = W::new().bytes(&locked).bytes(&[0xff, 0xfe]).0;
    assert!(Rd::error(&call(102, &bad)).is_some());
    // A node handle is refused: these ops name no node.
    let response = dispatch(&Registry::new(), &frame(100, 7, &W::new().bytes(b"").0));
    assert_eq!(refusal(&response).1, "handle-unexpected");
}
