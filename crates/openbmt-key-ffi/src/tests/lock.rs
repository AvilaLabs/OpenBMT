//! The PIN lock of the stored state (`docs/SIGNER-RULES.md`, SR-71 to
//! SR-73): the locked frame, the PIN rules, the older state that opens only
//! into set-a-PIN, and what a locked app refuses.

use std::sync::Arc;

use super::kit::*;
use super::wire::{frame, W};
use crate::lock::{self, KdfParams, HEADER_LEN, MAGIC_LOCKED};
use crate::{
    dispatch, EntropySource, KeyApp, KeyError, KeyNetwork, KeyStore, LockStatus, Registry,
    SetupState, StoreError,
};

/// A header with chosen numbers, then `body` bytes of "ciphertext".
fn header_with(m: u32, t: u32, p: u32, body: usize) -> Vec<u8> {
    let mut out = MAGIC_LOCKED.to_vec();
    out.push(1);
    out.extend_from_slice(&m.to_le_bytes());
    out.extend_from_slice(&t.to_le_bytes());
    out.extend_from_slice(&p.to_le_bytes());
    out.extend_from_slice(&[3u8; lock::SALT_LEN]);
    out.extend_from_slice(&[4u8; lock::NONCE_LEN]);
    out.extend_from_slice(&u32::try_from(body).expect("fits").to_le_bytes());
    out.extend_from_slice(&vec![5u8; body]);
    out
}

fn rule(r: Result<impl std::fmt::Debug, KeyError>) -> String {
    r.expect_err("refused").rule().to_string()
}

// ---- SR-71: the frame ------------------------------------------------------------------

#[test]
fn sr_71_the_standard_parameters_are_64_mib_3_passes_1_lane() {
    assert_eq!(
        KdfParams::STANDARD,
        KdfParams {
            m_kib: 64 * 1024,
            t: 3,
            p: 1
        }
    );
    assert_eq!(
        KdfParams::LIMIT,
        KdfParams {
            m_kib: 256 * 1024,
            t: 10,
            p: 4
        }
    );
    // The real cost, once: a frame under the standard parameters opens.
    let key =
        lock::LockKey::new(KdfParams::STANDARD, "482913", [9; lock::SALT_LEN]).expect("a key");
    let mut key = key;
    let sealed = key
        .seal_next([1; lock::NONCE_LEN], b"inside")
        .expect("sealed");
    let frame = lock::decode(&sealed).expect("decodes");
    assert_eq!(frame.header.params, KdfParams::STANDARD);
    let (plain, _) = lock::open(&sealed, "482913")
        .map_err(|_| ())
        .expect("opens");
    assert_eq!(plain, b"inside");
    assert!(lock::open(&sealed, "482914").is_err());
}

#[test]
fn sr_71_the_frame_reads_back_and_nothing_else_does() {
    let rig = rig();
    let bytes = rig.store.saved().expect("stored");
    assert_eq!(&bytes[..4], b"OBMx");
    assert_eq!(bytes[4], 1);
    let parsed = lock::decode(&bytes).expect("decodes");
    assert_eq!(parsed.to_bytes(), bytes);
    assert_eq!(parsed.header.to_bytes().len(), HEADER_LEN);

    // Every cut is refused (the length is in the header), and so is every
    // extra byte.
    for n in 0..bytes.len() {
        assert!(lock::decode(&bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(lock::decode(&longer).is_err());
    // Another magic and another version are refused.
    let mut other = bytes.clone();
    other[3] = b'7';
    assert!(lock::decode(&other).is_err());
    let mut other = bytes.clone();
    other[4] = 2;
    assert!(lock::decode(&other).is_err());

    // A flipped bit in the header never opens, whatever it does to the
    // parameters, the salt, the nonce or the length; one in the ciphertext
    // never opens either.
    for i in (0..bytes.len()).step_by(11).chain(0..HEADER_LEN) {
        for bit in [0, 3, 7] {
            let mut flipped = bytes.clone();
            flipped[i] ^= 1 << bit;
            assert!(lock::open(&flipped, PIN).is_err(), "byte {i} bit {bit}");
        }
    }
}

#[test]
fn sr_71_parameters_over_the_limit_are_refused_before_anything_runs() {
    let limit = KdfParams::LIMIT;
    let ok = header_with(limit.m_kib, limit.t, limit.p, 32);
    assert!(lock::decode(&ok).is_ok());
    for (m, t, p) in [
        (limit.m_kib + 1, limit.t, limit.p),
        (limit.m_kib, limit.t + 1, limit.p),
        (limit.m_kib, limit.t, limit.p + 1),
        (u32::MAX, 1, 1),
        (64, u32::MAX, 1),
        (64, 1, u32::MAX),
        // Not a thing Argon2 takes.
        (64, 0, 1),
        (64, 1, 0),
        (7, 1, 1),
        (8, 1, 2),
    ] {
        assert!(
            lock::decode(&header_with(m, t, p, 32)).is_err(),
            "{m} {t} {p}"
        );
    }
    // A file that asks for too much is never opened with a PIN either.
    let big = header_with(limit.m_kib + 1, 1, 1, 32);
    assert!(lock::open(&big, PIN).is_err());
    // Nor is a length that is not the bytes that follow, or a body shorter
    // than a tag.
    let mut bad = header_with(64, 1, 1, 32);
    bad.truncate(bad.len() - 1);
    assert!(lock::decode(&bad).is_err());
    assert!(lock::decode(&header_with(64, 1, 1, 15)).is_err());
    assert!(lock::decode(&header_with(64, 1, 1, 16)).is_ok());
    // The length field may not claim more than the file can hold.
    let mut huge = header_with(64, 1, 1, 16);
    huge[HEADER_LEN - 4..HEADER_LEN].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(lock::decode(&huge).is_err());
}

#[test]
fn sr_71_every_write_is_a_locked_frame_with_a_new_nonce() {
    let store = MemStore::new();
    let app = open_new(store.clone());
    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("a key");
    let mut seen = Vec::new();
    let mut last = store.saved().expect("stored");
    for k in 0..4 {
        let frame = lock::decode(&last).expect("a locked frame");
        seen.push((frame.header.nonce, frame.header.salt));
        // The same state written again is still a new frame.
        app.create_approver_only().expect("again");
        let next = store.saved().expect("stored");
        assert_ne!(next, last, "write {k}");
        last = next;
    }
    let frame = lock::decode(&last).expect("a locked frame");
    seen.push((frame.header.nonce, frame.header.salt));
    for (i, (nonce, salt)) in seen.iter().enumerate() {
        // The key is the same while the app is open, so the salt is, and
        // the nonce never is.
        assert_eq!(*salt, seen[0].1);
        for (j, (other, _)) in seen.iter().enumerate() {
            assert!(i == j || nonce != other, "writes {i} and {j}");
        }
    }
}

/// A store whose random source never changes.
struct SameRandom(Arc<MemStore>);

impl KeyStore for SameRandom {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        self.0.load()
    }

    fn save(&self, bytes: Vec<u8>) -> Result<(), StoreError> {
        self.0.save(bytes)
    }

    fn random(&self, n: u32) -> Result<Vec<u8>, StoreError> {
        Ok(vec![7; usize::try_from(n).expect("fits")])
    }

    fn now(&self) -> u64 {
        self.0.now()
    }
}

#[test]
fn sr_71_a_nonce_is_never_reused_even_when_the_random_source_repeats() {
    let inner = MemStore::new();
    let app = KeyApp::open(Arc::new(SameRandom(inner.clone()))).expect("opens");
    app.set_pin(PIN.to_string()).expect("a PIN");
    app.create_approver_only().expect("the first write");
    let first = inner.saved().expect("stored");
    // The second write would draw the same nonce under the same key: it is
    // refused, nothing is stored, and the app says its store failed.
    let err = app.create_approver_only().expect_err("refused");
    assert_eq!(err.rule(), "AS-34");
    assert_eq!(inner.saved().expect("stored"), first);
    assert_eq!(inner.saves(), 1);
}

// ---- SR-72: the PIN, and the older state -----------------------------------------------

#[test]
fn sr_72_nothing_is_stored_until_a_pin_is_set() {
    let store = MemStore::new();
    let app = KeyApp::open(store.clone()).expect("opens");
    assert_eq!(app.lock_status().expect("status"), LockStatus::NoPin);
    assert_eq!(app.state().expect("state"), SetupState::Empty);
    assert_eq!(
        rule(app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)),
        "pin-first"
    );
    assert_eq!(rule(app.create_approver_only()), "pin-first");
    assert_eq!(store.saves(), 0);
    app.set_pin(PIN.to_string()).expect("a PIN");
    assert_eq!(app.lock_status().expect("status"), LockStatus::Open);
    // Setting a PIN stores nothing: there is nothing to store yet.
    assert_eq!(store.saves(), 0);
    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("a key");
    assert_eq!(store.saves(), 1);
    assert!(lock::is_locked(&store.saved().expect("stored")));
    // A key restored from its words is made the same way.
    let restored = MemStore::new();
    let app = open_new(restored.clone());
    app.create_from_entropy(
        EntropySource::TypedWords {
            words: [
                "abandon", "abandon", "abandon", "abandon", "abandon", "abandon", "abandon",
                "abandon", "abandon", "abandon", "abandon", "about",
            ]
            .iter()
            .map(|w| (*w).to_string())
            .collect(),
        },
        KeyNetwork::Regtest,
    )
    .expect("restored");
    assert!(lock::is_locked(&restored.saved().expect("stored")));
}

#[test]
fn sr_72_a_pin_is_six_digits_or_a_passphrase_of_eight_characters() {
    let long = "x".repeat(128);
    let too_long = "x".repeat(129);
    let accepted = [
        "123456",
        "000000",
        "12345678901234567890",
        "abcdefgh",
        "correct horse",
        "pass word",
        "ÄÖÜäöüßé",
        long.as_str(),
    ];
    let weak = [
        "",
        "1",
        "12345",
        "abcdef",
        "abcdefg",
        "12345a",
        "ÄÖÜäö",
        "      ",
    ];
    for pin in accepted {
        let app = KeyApp::open(MemStore::new()).expect("opens");
        app.set_pin(pin.to_string())
            .unwrap_or_else(|e| panic!("{pin:?}: {e}"));
        assert_eq!(app.lock_status().expect("status"), LockStatus::Open);
    }
    for pin in weak {
        let app = KeyApp::open(MemStore::new()).expect("opens");
        assert_eq!(rule(app.set_pin(pin.to_string())), "pin-weak", "{pin:?}");
        assert_eq!(app.lock_status().expect("status"), LockStatus::NoPin);
    }
    let app = KeyApp::open(MemStore::new()).expect("opens");
    assert_eq!(rule(app.set_pin(too_long)), "pin-long");
    assert_eq!(app.lock_status().expect("status"), LockStatus::NoPin);
}

#[test]
fn sr_72_a_pin_is_set_once() {
    let store = MemStore::new();
    let app = open_new(store.clone());
    assert_eq!(rule(app.set_pin("654321".to_string())), "pin-set");
    // The first PIN still stands.
    app.create_approver_only().expect("a key");
    let again = KeyApp::open(store).expect("opens");
    assert_eq!(rule(again.unlock("654321".to_string())), "pin-wrong");
    again.unlock(PIN.to_string()).expect("the first PIN");
}

#[test]
fn sr_72_an_older_state_opens_only_into_set_a_pin() {
    let rig = rig();
    let signer_before = rig.app.signer_status().expect("status");
    let plain = rig.store.plain().expect("stored");
    let old = MemStore::new();
    old.set_saved(Some(plain.clone()));

    let app = KeyApp::open(old.clone()).expect("opens");
    assert_eq!(app.lock_status().expect("status"), LockStatus::Legacy);
    assert_eq!(app.state().expect("state"), SetupState::Locked);
    // Nothing but setting a PIN works, and each refusal says so.
    assert_eq!(rule(app.key2_public()), "pin-needed");
    assert_eq!(rule(app.signer_status()), "pin-needed");
    assert_eq!(rule(app.pair_offer_bytes()), "pin-needed");
    assert_eq!(rule(app.create_approver_only()), "pin-needed");
    assert_eq!(rule(app.unlock(PIN.to_string())), "not-locked");
    assert_eq!(
        rule(app.change_pin(PIN.to_string(), "654321".to_string())),
        "pin-needed"
    );
    // A PIN the rules do not take leaves it as it was.
    assert_eq!(rule(app.set_pin("1234".to_string())), "pin-weak");
    assert_eq!(app.lock_status().expect("status"), LockStatus::Legacy);
    assert_eq!(old.saved().expect("stored"), plain);

    // A store that fails leaves it as it was, and the PIN is not set.
    old.fail_saves(true);
    assert_eq!(rule(app.set_pin(PIN.to_string())), "AS-34");
    old.fail_saves(false);
    assert_eq!(app.lock_status().expect("status"), LockStatus::Legacy);
    assert_eq!(old.saved().expect("stored"), plain);

    // Setting the PIN stores the same state behind it, and goes on.
    app.set_pin(PIN.to_string()).expect("wrapped");
    assert!(lock::is_locked(&old.saved().expect("stored")));
    assert_eq!(app.lock_status().expect("status"), LockStatus::Open);
    assert_eq!(
        app.state().expect("state"),
        SetupState::Ready { holds_key2: true }
    );
    assert_eq!(app.signer_status().expect("status"), signer_before);
    // And from then on it is locked like any other.
    let again = KeyApp::open(old.clone()).expect("opens");
    assert_eq!(again.lock_status().expect("status"), LockStatus::Locked);
    again.unlock(PIN.to_string()).expect("unlocks");
    assert_eq!(again.signer_status().expect("status"), signer_before);
}

// ---- SR-73: locked, wrong PIN, change of PIN ----------------------------------------

#[test]
fn sr_73_a_locked_app_refuses_every_call_but_the_lock_s_own() {
    let rig = rig();
    let store = rig.store.clone();
    let app = KeyApp::open(store.clone()).expect("opens");
    assert_eq!(app.state().expect("state"), SetupState::Locked);
    assert_eq!(app.lock_status().expect("status"), LockStatus::Locked);
    let saved = store.saved();
    let saves = store.saves();

    let locked = |r: Result<(), KeyError>| assert_eq!(rule(r), "locked");
    locked(app.key_words().map(drop));
    locked(app.key2_public().map(drop));
    locked(app.device_root().map(drop));
    locked(app.device_root_words().map(drop));
    locked(app.wallet_view().map(drop));
    locked(app.pair_offer_bytes().map(drop));
    locked(app.check_kit(String::new()).map(drop));
    locked(app.signer_status().map(drop));
    locked(app.receive_mark().map(drop));
    locked(app.enroll_destination(address(5), "Exchange".to_string()));
    locked(app.remove_destination(address(5)));
    locked(app.raise_change_mark(5));
    locked(app.raise_receive_mark(5));
    locked(app.correct_clock());
    locked(app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest));
    locked(app.create_approver_only());
    locked(app.take_key3(String::new()).map(drop));
    locked(app.key3_view().map(drop));
    locked(app.public_answer().map(drop));
    locked(app.accept_wallet_setup(Vec::new()).map(drop));
    locked(app.confirm_wallet());
    locked(app.discard_wallet());
    locked(app.open_session(Vec::new()).map(drop));
    locked(app.set_pin(PIN.to_string()));
    locked(app.change_pin(PIN.to_string(), "654321".to_string()));
    // Nothing was stored or changed by any of them.
    assert_eq!(store.saved(), saved);
    assert_eq!(store.saves(), saves);
    assert_eq!(app.lock_status().expect("status"), LockStatus::Locked);

    // The same through the call protocol: the refusal is a state error
    // with the rule `locked`.
    let reg = Registry::new();
    let reply = |op: u16, handle: u64, fields: W| {
        let response = dispatch(&reg, store.clone(), &frame(op, handle, &fields.0));
        (response[5], response[6..].to_vec())
    };
    let (_, body) = reply(1, 0, W::new());
    let handle = u64::from_le_bytes(body[..8].try_into().expect("8"));
    assert_eq!(reply(10, handle, W::new()), (0, vec![4]));
    assert_eq!(reply(90, handle, W::new()), (0, vec![2]));
    let (status, body) = reply(40, handle, W::new());
    assert_eq!(status, 3);
    assert_eq!(&body[2..8], b"locked");
    let (status, _) = reply(11, handle, W::new().u8(0).u8(3));
    assert_eq!(status, 3);
}

#[test]
fn sr_73_a_wrong_pin_is_refused_and_changes_nothing() {
    let rig = rig();
    let store = rig.store.clone();
    let before = rig.app.signer_status().expect("status");
    let app = KeyApp::open(store.clone()).expect("opens");
    let saved = store.saved();
    let saves = store.saves();
    for wrong in ["", "482914", "48291", "x", "482913 ", "4829133"] {
        let err = app.unlock(wrong.to_string()).expect_err("refused");
        assert_eq!(err.rule(), "pin-wrong", "{wrong:?}");
        assert!(matches!(err, KeyError::Refused { .. }));
        assert_eq!(app.lock_status().expect("status"), LockStatus::Locked);
        assert_eq!(app.state().expect("state"), SetupState::Locked);
        assert_eq!(store.saved(), saved);
        assert_eq!(store.saves(), saves);
    }
    app.unlock(PIN.to_string()).expect("the right PIN");
    assert_eq!(app.lock_status().expect("status"), LockStatus::Open);
    assert_eq!(app.signer_status().expect("status"), before);
    // Unlocking again is refused, and still changes nothing.
    assert_eq!(rule(app.unlock(PIN.to_string())), "not-locked");
    assert_eq!(store.saves(), saves);
}

#[test]
fn sr_73_a_changed_pin_is_one_rewrite_and_the_old_one_stops_working() {
    let rig = rig();
    let store = rig.store.clone();
    let before = rig.app.signer_status().expect("status");
    let saves = store.saves();
    let saved = store.saved();

    // A wrong current PIN, and a new one the rules do not take, change
    // nothing.
    assert_eq!(
        rule(
            rig.app
                .change_pin("111111".to_string(), "654321".to_string())
        ),
        "pin-wrong"
    );
    assert_eq!(
        rule(rig.app.change_pin(PIN.to_string(), "1234".to_string())),
        "pin-weak"
    );
    assert_eq!(store.saved(), saved);
    assert_eq!(store.saves(), saves);

    // A store that fails changes nothing, and the old PIN still stands.
    store.fail_saves(true);
    assert_eq!(
        rule(rig.app.change_pin(PIN.to_string(), "654321".to_string())),
        "AS-34"
    );
    store.fail_saves(false);
    assert_eq!(store.saved(), saved);

    rig.app
        .change_pin(PIN.to_string(), "654321".to_string())
        .expect("changed");
    assert_eq!(store.saves(), saves + 1, "one rewrite");
    assert_ne!(store.saved(), saved);
    // The app goes on with the new key: its next write is under it.
    rig.app.raise_receive_mark(7).expect("a write");
    assert_eq!(store.saves(), saves + 2);

    let again = KeyApp::open(store.clone()).expect("opens");
    assert_eq!(rule(again.unlock(PIN.to_string())), "pin-wrong");
    again.unlock("654321".to_string()).expect("the new PIN");
    let after = again.signer_status().expect("status");
    assert_eq!(after.receive_mark, 7);
    assert_eq!(after.allowlist, before.allowlist);

    // The PIN can be a passphrase, and changed back.
    again
        .change_pin("654321".to_string(), "a long passphrase".to_string())
        .expect("changed");
    let third = KeyApp::open(store).expect("opens");
    third
        .unlock("a long passphrase".to_string())
        .expect("opens");
}

#[test]
fn sr_73_a_pin_can_be_changed_before_a_key_is_made() {
    let store = MemStore::new();
    let app = open_new(store.clone());
    app.change_pin(PIN.to_string(), "654321".to_string())
        .expect("changed");
    assert_eq!(store.saves(), 0);
    app.create_approver_only().expect("a key");
    let again = KeyApp::open(store).expect("opens");
    assert_eq!(rule(again.unlock(PIN.to_string())), "pin-wrong");
    again.unlock("654321".to_string()).expect("the new PIN");
}

#[test]
fn sr_73_a_stored_frame_that_cannot_be_read_is_not_a_wrong_pin() {
    let rig = rig();
    let bytes = rig.store.saved().expect("stored");
    // Cut or lengthened: refused when the app is opened, with the rule of
    // the frame and not the rule of a wrong PIN.
    for broken in [
        bytes[..bytes.len() - 1].to_vec(),
        bytes[..HEADER_LEN].to_vec(),
        [bytes.clone(), vec![0]].concat(),
    ] {
        let store = MemStore::new();
        store.set_saved(Some(broken));
        let err = KeyApp::open(store).err().expect("refused");
        assert!(matches!(err, KeyError::Store { .. }));
        assert_eq!(err.rule(), "OBMx");
    }
    // A flipped byte of the ciphertext is the same answer as a wrong PIN:
    // the tag does not verify.
    let mut flipped = bytes;
    let at = flipped.len() - 5;
    flipped[at] ^= 1;
    let store = MemStore::new();
    store.set_saved(Some(flipped));
    let app = KeyApp::open(store).expect("opens");
    assert_eq!(rule(app.unlock(PIN.to_string())), "pin-wrong");
}

#[test]
fn sr_73_the_pin_is_not_in_the_stored_bytes_and_the_state_is_not_either() {
    let rig = rig();
    let stored = rig.store.saved().expect("stored");
    let plain = rig.store.plain().expect("stored");
    let hay = String::from_utf8_lossy(&stored).into_owned();
    assert!(!hay.contains(PIN));
    // No run of the plain bytes (the identity secret is at 5..37) is in the
    // locked frame.
    let secret = &plain[5..37];
    assert!(!stored.windows(32).any(|w| w == secret));
    assert!(!stored.windows(4).skip(HEADER_LEN).any(|w| w == b"OBM3"));
}
