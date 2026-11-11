//! The lock on the owner phone's setup text: ops 100 to 103
//! (`docs/OWNER-APP-RULES.md` OA-60 to OA-62, `docs/LOCK-RULES.md`).
//!
//! The setup text holds the owner's secret and key 1's account key. The app
//! stores `OBMx(setup text)` sealed by the Android Keystore, so that the
//! phone and the PIN are both needed. These ops lock a text under a PIN,
//! open a locked one and change the PIN in one step, all through
//! `openbmt-lock`; they work on no node and store nothing: the app stores
//! what they return. The salt and the nonce are read straight from the
//! system's random source, with no fallback.

use openbmt_lock::{self as lock, LockError, OpenError, PinRefusal, NONCE_LEN, SALT_LEN};

use crate::codec::{wire_record, wire_record_get};
use crate::error::{internal, refused_by, AppError};

/// What `setup_change_pin` is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinChangeArgs {
    /// The locked setup as the app stores it, inside the Keystore's seal.
    pub locked: Vec<u8>,
    /// The PIN that opens it now.
    pub current: String,
    /// The PIN it is locked under from now on.
    pub new: String,
}

wire_record_get!(PinChangeArgs {
    locked,
    current,
    new
});
wire_record!(PinChangeArgs {
    locked,
    current,
    new
});

/// Overwrite a PIN the call was given.
fn wipe(pin: String) {
    let mut bytes = pin.into_bytes();
    bytes.fill(0);
    std::hint::black_box(&mut bytes);
}

/// `N` bytes read from the system's random source. A read that fails or comes
/// up short is refused.
fn random<const N: usize>() -> Result<[u8; N], AppError> {
    random_from("/dev/urandom")
}

fn random_from<const N: usize>(path: &str) -> Result<[u8; N], AppError> {
    use std::io::Read;
    let mut out = [0u8; N];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut out))
        .map_err(|_| {
            refused_by(
                "pin-random",
                "the system's random source did not give the bytes for a lock",
            )
        })?;
    Ok(out)
}

/// A PIN the rules of `openbmt-lock` do not take, with the rule string this
/// layer has words for.
fn pin_refused(p: PinRefusal) -> AppError {
    match p.rule {
        "pin-long" => refused_by("pin-long", p.text),
        _ => refused_by("pin-weak", p.text),
    }
}

fn refusal(e: LockError) -> AppError {
    match e {
        LockError::Pin(p) => pin_refused(p),
        LockError::WrongPin => wrong_pin(),
        LockError::Unreadable(e) => unreadable(e),
        LockError::Fault(text) => internal(text),
    }
}

fn wrong_pin() -> AppError {
    refused_by("pin-wrong", "the PIN is wrong; nothing was changed")
}

fn unreadable(e: impl std::fmt::Display) -> AppError {
    refused_by(
        "OBMx",
        format!("OBMx: the locked setup is not readable: {e}"),
    )
}

/// Op 100: whether the stored bytes are a locked setup (`OBMx`). A setup
/// stored before the lock existed is the bare text, which opens only into
/// the step that sets a PIN.
pub(crate) fn form(bytes: Vec<u8>) -> Result<bool, AppError> {
    Ok(lock::is_locked(&bytes))
}

/// Op 101: lock the setup text under a PIN, or a long passphrase.
pub(crate) fn lock_text((pin, text): (String, Vec<u8>)) -> Result<Vec<u8>, AppError> {
    let result = lock_with(&pin, &text);
    wipe(pin);
    result
}

fn lock_with(pin: &str, text: &[u8]) -> Result<Vec<u8>, AppError> {
    // A PIN the rules do not take is refused before the random source is
    // read or Argon2 runs.
    lock::check_pin(pin).map_err(pin_refused)?;
    let salt = random::<SALT_LEN>()?;
    let nonce = random::<NONCE_LEN>()?;
    lock::lock(lock::cost!(), pin, salt, nonce, text).map_err(refusal)
}

/// Op 102: the setup text inside a locked setup. A wrong PIN is refused with
/// the rule `pin-wrong` and changes nothing.
pub(crate) fn unlock_text((locked, pin): (Vec<u8>, String)) -> Result<Vec<u8>, AppError> {
    let result = lock::unlock(&locked, &pin).map_err(|e| match e {
        OpenError::WrongPin => wrong_pin(),
        OpenError::Unreadable(e) => unreadable(e),
    });
    wipe(pin);
    result
}

/// Op 103: the locked setup under another PIN, in one call: the current PIN
/// opens it and the same text is locked again under the new PIN, a new salt
/// and a new nonce. A wrong current PIN is refused with the rule `pin-wrong`.
pub(crate) fn change_pin(a: PinChangeArgs) -> Result<Vec<u8>, AppError> {
    let PinChangeArgs {
        locked,
        current,
        new,
    } = a;
    let result = change_with(&locked, &current, &new);
    wipe(current);
    wipe(new);
    result
}

fn change_with(locked: &[u8], current: &str, new: &str) -> Result<Vec<u8>, AppError> {
    // The frame is read first, so that bytes that are not one are told apart
    // from a PIN that is wrong.
    lock::decode(locked).map_err(unreadable)?;
    lock::check_pin(new).map_err(pin_refused)?;
    let salt = random::<SALT_LEN>()?;
    let nonce = random::<NONCE_LEN>()?;
    lock::change(locked, current, lock::cost!(), new, salt, nonce).map_err(refusal)
}

#[cfg(test)]
mod random_tests {
    use super::*;

    #[test]
    fn oa_60_a_failing_random_source_is_refused() {
        let nothing = random_from::<16>("/nonexistent/random").expect_err("no source");
        assert_eq!(nothing.rule(), "pin-random");
        let short = random_from::<16>("/dev/null").expect_err("short");
        assert_eq!(short.rule(), "pin-random");
        assert_ne!(random::<16>().expect("urandom"), [0u8; 16]);
    }
}
