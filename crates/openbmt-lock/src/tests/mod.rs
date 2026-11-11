//! The tests of `docs/LOCK-RULES.md`, each named for its rule, and the check
//! that every rule has one.

use openbmt_core::wire::DecodeError;

use crate::{
    change, check_pin, cost, decode, is_locked, lock, open, reseal, unlock, KdfParams, LockError,
    LockKey, OpenError, HEADER_LEN, MAGIC_LOCKED, MAX_PIN_CHARS, NONCE_LEN, SALT_LEN,
};

const RULES: &str = include_str!("../../../../docs/LOCK-RULES.md");
const PIN: &str = "482913";

/// A frame made by the Key app's code before this crate existed: the PIN
/// `482913`, the parameters 64 KiB, 1 pass, 1 lane (the cost of its tests),
/// the salt of nine bytes, the nonce of one bytes, and the text
/// `the setup text` inside.
const BEFORE_THE_MOVE: &str = "4f424d7801400000000100000001000000090909090909090909090909090909090101010101010101010101011e000000a92c7ed31c5e9f1db9b0981dc7ff49e8359418a9b3f2afffbc145844eb8c";

fn from_hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

fn header_with(m: u32, t: u32, p: u32, body: usize) -> Vec<u8> {
    let mut out = MAGIC_LOCKED.to_vec();
    out.push(1);
    out.extend_from_slice(&m.to_le_bytes());
    out.extend_from_slice(&t.to_le_bytes());
    out.extend_from_slice(&p.to_le_bytes());
    out.extend_from_slice(&[3u8; SALT_LEN]);
    out.extend_from_slice(&[4u8; NONCE_LEN]);
    out.extend_from_slice(&u32::try_from(body).expect("fits").to_le_bytes());
    out.extend_from_slice(&vec![5u8; body]);
    out
}

fn locked(plain: &[u8]) -> Vec<u8> {
    lock(cost!(), PIN, [7; SALT_LEN], [8; NONCE_LEN], plain).expect("locks")
}

#[test]
fn lc_01_a_frame_written_before_the_move_still_opens() {
    let bytes = from_hex(BEFORE_THE_MOVE);
    assert_eq!(bytes.len(), HEADER_LEN + 30);
    let frame = decode(&bytes).expect("decodes");
    assert_eq!(frame.header.params, KdfParams::CHEAP);
    assert_eq!(frame.to_bytes(), bytes);
    assert_eq!(unlock(&bytes, PIN).expect("opens"), b"the setup text");
    assert!(unlock(&bytes, "482914").is_err());
    // The code that writes it still writes the same bytes.
    let mut key = LockKey::new(KdfParams::CHEAP, PIN, [9; SALT_LEN]).expect("a key");
    let again = key
        .seal_next([1; NONCE_LEN], b"the setup text")
        .expect("sealed");
    assert_eq!(again, bytes);
}

#[test]
fn lc_01_the_frame_reads_back_and_nothing_else_does() {
    let bytes = locked(b"inside");
    assert_eq!(&bytes[..4], b"OBMx");
    assert_eq!(bytes[4], 1);
    assert!(is_locked(&bytes) && !is_locked(b"OBM7") && !is_locked(b"OB"));
    let parsed = decode(&bytes).expect("decodes");
    assert_eq!(parsed.to_bytes(), bytes);
    assert_eq!(parsed.header.to_bytes().len(), HEADER_LEN);
    for n in 0..bytes.len() {
        assert!(decode(&bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(decode(&longer).is_err());
    let mut other = bytes.clone();
    other[3] = b'7';
    assert!(decode(&other).is_err());
    let mut other = bytes.clone();
    other[4] = 2;
    assert!(decode(&other).is_err());
    // A ciphertext shorter than the tag, and a length that is not the bytes
    // that follow.
    assert!(decode(&header_with(64, 1, 1, 15)).is_err());
    assert!(decode(&header_with(64, 1, 1, 16)).is_ok());
    let mut bad = header_with(64, 1, 1, 32);
    bad[HEADER_LEN - 4..HEADER_LEN].copy_from_slice(&33u32.to_le_bytes());
    assert!(decode(&bad).is_err());
    let mut huge = header_with(64, 1, 1, 32);
    huge[HEADER_LEN - 4..HEADER_LEN].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode(&huge).is_err());
    // Plain text is not a frame.
    assert!(matches!(
        decode(b"owner_secret=00"),
        Err(DecodeError::BadMagic | DecodeError::Truncated)
    ));
}

#[test]
fn lc_02_parameters_over_the_limit_are_refused_before_anything_runs() {
    assert_eq!(
        KdfParams::STANDARD,
        KdfParams {
            m_kib: 64 * 1024,
            t: 3,
            p: 1
        }
    );
    let limit = KdfParams::LIMIT;
    assert_eq!(
        limit,
        KdfParams {
            m_kib: 256 * 1024,
            t: 10,
            p: 4
        }
    );
    assert!(decode(&header_with(limit.m_kib, limit.t, limit.p, 32)).is_ok());
    for (m, t, p) in [
        (limit.m_kib + 1, limit.t, limit.p),
        (limit.m_kib, limit.t + 1, limit.p),
        (limit.m_kib, limit.t, limit.p + 1),
        (u32::MAX, 1, 1),
        (64, 0, 1),
        (64, 1, 0),
        (7, 1, 1),
        (8, 1, 2),
    ] {
        assert!(decode(&header_with(m, t, p, 32)).is_err(), "{m} {t} {p}");
    }
    assert!(open(&header_with(limit.m_kib + 1, 1, 1, 32), PIN).is_err());
    assert!(LockKey::new(
        KdfParams {
            m_kib: 64,
            t: 0,
            p: 1
        },
        PIN,
        [0; SALT_LEN]
    )
    .is_none());
    // The real cost, once: a frame under the standard parameters opens.
    let mut key = LockKey::new(KdfParams::STANDARD, PIN, [9; SALT_LEN]).expect("a key");
    let sealed = key.seal_next([1; NONCE_LEN], b"inside").expect("sealed");
    assert_eq!(
        decode(&sealed).expect("decodes").header.params,
        KdfParams::STANDARD
    );
    assert_eq!(unlock(&sealed, PIN).expect("opens"), b"inside");
}

#[test]
fn lc_03_a_pin_is_six_digits_or_a_passphrase_of_eight() {
    for ok in ["482913", "1234567890", "abcdefgh", "ab cd efg", "ünïcödé!"] {
        assert!(check_pin(ok).is_ok(), "{ok}");
    }
    for weak in ["", "48291", "abcdefg", "12345a", "     "] {
        assert_eq!(check_pin(weak).expect_err(weak).rule, "pin-weak", "{weak}");
    }
    // Eight characters, more than eight bytes.
    assert!(check_pin("ééééééé").is_err() && check_pin("éééééééé").is_ok());
    assert!(check_pin(&"9".repeat(MAX_PIN_CHARS)).is_ok());
    assert_eq!(
        check_pin(&"9".repeat(MAX_PIN_CHARS + 1))
            .expect_err("long")
            .rule,
        "pin-long"
    );
    assert!(check_pin(&"é".repeat(MAX_PIN_CHARS)).is_ok());
    assert!(check_pin(&"é".repeat(MAX_PIN_CHARS + 1)).is_err());
    // A PIN that is refused locks nothing.
    let refused = lock(cost!(), "123", [1; SALT_LEN], [2; NONCE_LEN], b"x");
    assert!(matches!(refused, Err(LockError::Pin(p)) if p.rule == "pin-weak"));
}

#[test]
fn lc_04_a_nonce_is_never_repeated_under_one_key() {
    let mut key = LockKey::new(cost!(), PIN, [1; SALT_LEN]).expect("a key");
    let first = key.seal_next([5; NONCE_LEN], b"one").expect("first");
    assert!(key.seal_next([5; NONCE_LEN], b"two").is_none());
    let second = key.seal_next([6; NONCE_LEN], b"two").expect("second");
    assert_ne!(first, second);
    // A key that read a frame does not repeat that frame's nonce either.
    let (_, mut read) = open(&first, PIN).map_err(|_| ()).expect("opens");
    assert!(read.seal_next([5; NONCE_LEN], b"three").is_none());
    assert!(read.seal_next([9; NONCE_LEN], b"three").is_some());
}

#[test]
fn lc_05_a_wrong_pin_and_a_changed_frame_give_the_same_answer() {
    let bytes = locked(b"inside");
    assert!(matches!(unlock(&bytes, "482914"), Err(OpenError::WrongPin)));
    // Every byte of the header and a spread of the ciphertext, flipped.
    for i in (0..bytes.len()).step_by(7).chain(0..HEADER_LEN) {
        for bit in [0, 3, 7] {
            let mut flipped = bytes.clone();
            flipped[i] ^= 1 << bit;
            assert!(unlock(&flipped, PIN).is_err(), "byte {i} bit {bit}");
        }
    }
    // A frame that is not one is told apart from a wrong PIN.
    assert!(matches!(
        unlock(&bytes[..10], PIN),
        Err(OpenError::Unreadable(_))
    ));
    let key = LockKey::new(cost!(), PIN, [1; SALT_LEN]).expect("a key");
    assert!(key.matches(PIN) && !key.matches("482914"));
    assert_eq!(format!("{key:?}"), "LockKey(<hidden>)");
    // The source overwrites the derived key when the key is dropped.
    let src = include_str!("../lib.rs");
    assert!(src.contains("self.key.fill(0);") && src.contains("other.fill(0);"));
}

#[test]
fn lc_06_a_change_of_pin_is_one_step_and_needs_the_current_pin() {
    let bytes = locked(b"the setup text");
    let new = change(
        &bytes,
        PIN,
        cost!(),
        "a new passphrase",
        [11; SALT_LEN],
        [12; NONCE_LEN],
    )
    .expect("changed");
    assert_ne!(decode(&new).expect("a frame").header.salt, [7; SALT_LEN]);
    assert_eq!(
        unlock(&new, "a new passphrase").expect("opens"),
        b"the setup text"
    );
    assert!(unlock(&new, PIN).is_err());
    // The old PIN still opens the old frame: nothing was changed there.
    assert!(unlock(&bytes, PIN).is_ok());
    // A wrong current PIN, a weak new one, a frame that is not one and a
    // repeated salt or nonce return nothing.
    let go = |current: &str, new: &str, salt: u8, nonce: u8| {
        change(
            &bytes,
            current,
            cost!(),
            new,
            [salt; SALT_LEN],
            [nonce; NONCE_LEN],
        )
    };
    assert!(matches!(
        go("482914", "654321", 1, 2),
        Err(LockError::WrongPin)
    ));
    assert!(matches!(go(PIN, "654", 1, 2), Err(LockError::Pin(_))));
    assert!(matches!(go(PIN, "654321", 7, 2), Err(LockError::Fault(_))));
    assert!(matches!(go(PIN, "654321", 1, 8), Err(LockError::Fault(_))));
    assert!(matches!(
        change(
            b"plain",
            PIN,
            cost!(),
            "654321",
            [1; SALT_LEN],
            [2; NONCE_LEN]
        ),
        Err(LockError::Unreadable(_))
    ));
    // The same through the keys: the frame again under another key.
    let (_, old) = open(&bytes, PIN).map_err(|_| ()).expect("opens");
    let mut other = LockKey::new(cost!(), "654321", [3; SALT_LEN]).expect("a key");
    let resealed = reseal(&bytes, &old, &mut other, [4; NONCE_LEN]).expect("resealed");
    assert_eq!(
        unlock(&resealed, "654321").expect("opens"),
        b"the setup text"
    );
    let mut wrong = LockKey::new(cost!(), "000000", [3; SALT_LEN]).expect("a key");
    assert!(reseal(&bytes, &wrong, &mut other, [5; NONCE_LEN]).is_err());
    assert!(wrong.seal_next([1; NONCE_LEN], b"x").is_some());
}

#[test]
fn lc_07_the_cheap_cost_is_only_compiled_in_under_test_and_fuzzing() {
    let src = include_str!("../lib.rs");
    let gate = "#[cfg(not(any(test, fuzzing)))]\n        let params = $crate::KdfParams::STANDARD;\n        #[cfg(any(test, fuzzing))]\n        let params = $crate::KdfParams::CHEAP;";
    assert!(
        src.contains(gate),
        "the standard cost is the one outside test and fuzzing"
    );
    // Nothing else in the crate names the cheap parameters but the macro
    // and its own definition.
    assert_eq!(src.matches("KdfParams::CHEAP").count(), 1);
    assert_eq!(src.matches("pub const CHEAP").count(), 1);
    // Under this build the macro gives the cheap cost.
    assert_eq!(cost!(), KdfParams::CHEAP);
    assert_eq!(KdfParams::CHEAP.m_kib, 64);
}

/// The rule IDs that the rules file defines.
fn rule_ids() -> Vec<String> {
    RULES
        .lines()
        .filter_map(|l| l.strip_prefix("- **LC-"))
        .map(|rest| format!("LC-{}", &rest[..2]))
        .collect()
}

#[test]
fn the_rules_file_defines_its_rules_and_each_has_a_test() {
    let ids = rule_ids();
    assert_eq!(ids.len(), 7, "{ids:?}");
    assert_eq!(
        ids.iter()
            .map(|i| i[3..].parse::<u32>().expect("a number"))
            .collect::<Vec<_>>(),
        (1..=7).collect::<Vec<_>>()
    );
    let own = include_str!("mod.rs");
    for id in &ids {
        assert!(
            own.contains(&format!("fn lc_{}_", &id[3..])),
            "{id} has no test"
        );
    }
    for line in RULES.lines() {
        assert!(line.len() <= 80, "a line of the rules is too long: {line}");
    }
    assert!(RULES.contains("## 4. What these rules do not do"));
    assert!(RULES.lines().any(|l| l.starts_with("Status:")));
}
