//! The lock of the stored state: set a PIN, unlock, change the PIN
//! (`docs/SIGNER-RULES.md` SR-72, SR-73). The counting of wrong tries and the
//! wipe belong to the app around this library (SR-75, SR-76).

use crate::app::{phase_of, Gate, Inner, KeyApp, LockStatus, Wiped};
use crate::envelope::wipe_string;
use crate::error::{internal, refused_by, store_failed, wrong_state, KeyError};
use crate::keys::random_bytes;
use crate::lock::{self, LockKey, OpenError, NONCE_LEN, SALT_LEN};

fn wrong_pin() -> KeyError {
    refused_by("pin-wrong", "the PIN is wrong; nothing was changed")
}

fn unusable() -> KeyError {
    internal("the lock of the stored state is not usable")
}

fn unreadable(e: impl std::fmt::Display) -> KeyError {
    KeyError::Store {
        rule: "OBMx".to_string(),
        text: format!("OBMx: the stored state is not readable: {e}"),
    }
}

/// A key for `pin` under a new salt from the platform.
fn new_key(inner: &Inner, pin: &str) -> Result<LockKey, KeyError> {
    let bytes = random_bytes(&inner.raw, SALT_LEN)?;
    let salt = <[u8; SALT_LEN]>::try_from(bytes.as_slice())
        .map_err(|_| internal("the salt is not 16 bytes"))?;
    LockKey::new(lock::COST, pin, salt).ok_or_else(|| internal("the key could not be made"))
}

fn install(inner: &Inner, key: Option<LockKey>) -> Result<(), KeyError> {
    *inner.lock.lock().map_err(|_| unusable())? = key;
    Ok(())
}

fn key_held(inner: &Inner) -> bool {
    inner.lock.lock().map(|g| g.is_some()).unwrap_or(false)
}

impl KeyApp {
    /// Where the lock is: no PIN yet, an older state to wrap, locked, or
    /// open.
    pub fn lock_status(&self) -> Result<LockStatus, KeyError> {
        self.run_locked(|inner| {
            Ok(match &inner.gate {
                Gate::Locked(_) => LockStatus::Locked,
                Gate::Legacy(_) => LockStatus::Legacy,
                Gate::Open if key_held(inner) => LockStatus::Open,
                Gate::Open => LockStatus::NoPin,
            })
        })
    }

    /// SR-72: set the PIN, or the long passphrase, of a Key app that has
    /// none: before a new or restored key is made (nothing is stored yet),
    /// or for an older stored state, which is then stored again behind the
    /// PIN and opened. Refused when a PIN is set, and for a PIN the rules
    /// do not take.
    pub fn set_pin(&self, pin: String) -> Result<(), KeyError> {
        let result = self.run_locked(|inner| {
            if matches!(inner.gate, Gate::Locked(_)) {
                inner.require_open()?;
            }
            inner.no_session()?;
            lock::check_pin(&pin)?;
            if key_held(inner) {
                return Err(wrong_state(
                    "pin-set",
                    "a PIN is set already: change it instead",
                ));
            }
            let key = new_key(inner, &pin)?;
            let legacy = match &inner.gate {
                Gate::Legacy(w) => Some(Wiped::copy_of(w)),
                _ => None,
            };
            install(inner, Some(key))?;
            let Some(legacy) = legacy else {
                return Ok(());
            };
            // Built and stored before it replaces anything.
            let stored = phase_of(legacy.bytes()).and_then(|phase| {
                inner
                    .store
                    .save(legacy.bytes().to_vec())
                    .map_err(|e| store_failed(&e))
                    .map(|()| phase)
            });
            match stored {
                Ok(phase) => {
                    inner.phase = phase;
                    inner.gate = Gate::Open;
                    Ok(())
                }
                Err(e) => {
                    install(inner, None)?;
                    Err(e)
                }
            }
        });
        wipe_string(pin);
        result
    }

    /// SR-73: open the stored state with the PIN. A wrong PIN is refused
    /// with the rule `pin-wrong` and changes nothing.
    pub fn unlock(&self, pin: String) -> Result<(), KeyError> {
        let result = self.run_locked(|inner| {
            let Gate::Locked(stored) = &inner.gate else {
                return Err(wrong_state("not-locked", "the Key app is not locked"));
            };
            let (mut plain, key) = match lock::open(stored, &pin) {
                Ok(opened) => opened,
                Err(OpenError::WrongPin) => return Err(wrong_pin()),
                Err(OpenError::Unreadable(e)) => return Err(unreadable(e)),
            };
            let phase = phase_of(&plain);
            plain.fill(0);
            std::hint::black_box(&mut plain);
            let phase = phase?;
            install(inner, Some(key))?;
            inner.phase = phase;
            inner.gate = Gate::Open;
            Ok(())
        });
        wipe_string(pin);
        result
    }

    /// SR-73: change the PIN. The current PIN is needed; the stored state is
    /// written once, under the new PIN. A wrong current PIN is refused with
    /// the rule `pin-wrong` and changes nothing.
    pub fn change_pin(&self, current: String, new: String) -> Result<(), KeyError> {
        let result = self.run_locked(|inner| {
            inner.require_open()?;
            inner.no_session()?;
            let mut held = inner.lock.lock().map_err(|_| unusable())?;
            let old = held
                .as_ref()
                .ok_or_else(|| wrong_state("pin-first", "no PIN is set: set one first"))?;
            if !old.matches(&current) {
                return Err(wrong_pin());
            }
            lock::check_pin(&new)?;
            let mut key = new_key(inner, &new)?;
            let stored = inner.raw.load().map_err(|e| KeyError::Store {
                rule: "load".to_string(),
                text: format!("could not read the stored state: {e}"),
            })?;
            if let Some(stored) = stored {
                let nonce = random_bytes(&inner.raw, NONCE_LEN)?;
                let nonce = <[u8; NONCE_LEN]>::try_from(nonce.as_slice())
                    .map_err(|_| internal("the nonce is not 12 bytes"))?;
                let frame = lock::reseal(&stored, old, &mut key, nonce).map_err(|e| match e {
                    OpenError::WrongPin => internal("the stored state is not under the held PIN"),
                    OpenError::Unreadable(e) => unreadable(e),
                })?;
                inner.raw.save(frame).map_err(|e| store_failed(&e))?;
            }
            *held = Some(key);
            Ok(())
        });
        wipe_string(current);
        wipe_string(new);
        result
    }
}
