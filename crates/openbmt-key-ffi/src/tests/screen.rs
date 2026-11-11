//! The Key app's own screens, and containment of a panic.

use std::sync::Arc;

use bitcoin::Address;
use openbmt_signer::ALLOWLIST_WAIT_SECS;

use super::kit::*;
use crate::{CeilingsView, FeeCeilingsView, KeyApp, KeyError, SetupState};

#[test]
fn a_destination_is_enrolled_listed_and_removed_through_the_store() {
    let rig = rig();
    let saves = rig.store.saves();
    let status = rig.app.signer_status().expect("status");
    assert!(status.allowlist.is_empty());
    assert_eq!(status.allowlist_wait_secs, ALLOWLIST_WAIT_SECS);

    rig.app
        .enroll_destination(address(5), "Exchange".to_string())
        .expect("enrolled");
    assert_eq!(rig.store.saves(), saves + 1);
    let status = rig.app.signer_status().expect("status");
    assert_eq!(status.allowlist.len(), 1);
    let entry = &status.allowlist[0];
    assert_eq!(entry.label, "Exchange");
    assert_eq!(entry.address.as_deref(), Some(address(5).as_str()));
    assert_eq!(entry.enrolled_at, NOW);
    assert_eq!(entry.active_at, Some(NOW + ALLOWLIST_WAIT_SECS));
    assert_eq!(status.latest_time_seen, NOW);

    // It is kept across a restart.
    let again = reopen(rig.store.clone());
    assert_eq!(again.signer_status().expect("status"), status);

    // The rules refuse a repeat, a bad label and another network, by ID.
    let err = rig
        .app
        .enroll_destination(address(5), "Again".to_string())
        .expect_err("twice");
    assert_eq!(err.rule(), "SR-30");
    let err = rig
        .app
        .enroll_destination(address(6), String::new())
        .expect_err("label");
    assert_eq!(err.rule(), "SR-30");
    let mainnet = Address::from_script(&external(7), bitcoin::Network::Bitcoin)
        .expect("an address")
        .to_string();
    assert_eq!(
        rig.app
            .enroll_destination(mainnet, "Mainnet".to_string())
            .expect_err("network")
            .rule(),
        "address"
    );
    assert_eq!(
        rig.app
            .enroll_destination("not an address".to_string(), "X".to_string())
            .expect_err("text")
            .rule(),
        "address"
    );
    assert_eq!(rig.store.saves(), saves + 1);

    rig.app.remove_destination(address(5)).expect("removed");
    assert!(rig
        .app
        .signer_status()
        .expect("status")
        .allowlist
        .is_empty());
    assert_eq!(
        rig.app
            .remove_destination(address(5))
            .expect_err("gone")
            .rule(),
        "SR-30"
    );
}

#[test]
fn a_store_that_fails_leaves_the_signer_as_it_was() {
    let rig = rig();
    let before = rig.app.signer_status().expect("status");
    let bytes = rig.store.saved().expect("stored");
    rig.store.fail_saves(true);
    let err = rig
        .app
        .enroll_destination(address(5), "Exchange".to_string())
        .expect_err("fails");
    assert!(matches!(err, KeyError::Store { .. }));
    assert!(rig
        .app
        .set_ceilings(
            CeilingsView {
                per_tx: 1,
                per_day: 2,
                per_week: 3
            },
            CeilingsView {
                per_tx: 4,
                per_day: 5,
                per_week: 6
            },
        )
        .is_err());
    assert!(rig.app.raise_receive_mark(50).is_err());
    assert_eq!(rig.app.signer_status().expect("status"), before);
    assert_eq!(rig.store.saved().expect("stored"), bytes);
    rig.store.fail_saves(false);
    rig.app.raise_receive_mark(50).expect("now it works");
}

#[test]
fn ceilings_fee_ceilings_and_marks_are_the_signers_own() {
    let rig = rig();
    let other = CeilingsView {
        per_tx: 50_000,
        per_day: 150_000,
        per_week: 400_000,
    };
    let allowlisted = CeilingsView {
        per_tx: 1_000_000,
        per_day: 2_000_000,
        per_week: 5_000_000,
    };
    rig.app.set_ceilings(other, allowlisted).expect("set");
    rig.app
        .set_fee_ceilings(FeeCeilingsView {
            sat_per_vb: 80,
            absolute_sats: 100_000,
        })
        .expect("set");
    rig.app.raise_change_mark(10).expect("raised");
    rig.app.raise_receive_mark(30).expect("raised");
    let status = rig.app.signer_status().expect("status");
    assert_eq!(status.ceilings_other, other);
    assert_eq!(status.ceilings_allowlisted, allowlisted);
    assert_eq!(
        status.fee_ceilings,
        FeeCeilingsView {
            sat_per_vb: 80,
            absolute_sats: 100_000
        }
    );
    assert_eq!(status.change_mark, 10);
    assert_eq!(status.receive_mark, 30);
    assert_eq!(rig.app.receive_mark().expect("mark"), 30);

    // The signer's own bounds refuse, and name their rules.
    assert_eq!(
        rig.app
            .set_fee_ceilings(FeeCeilingsView {
                sat_per_vb: 5_000,
                absolute_sats: 10
            })
            .expect_err("bounds")
            .rule(),
        "SR-40"
    );
    assert_eq!(
        rig.app
            .raise_change_mark(10)
            .expect_err("not higher")
            .rule(),
        "SR-34"
    );
    assert_eq!(
        rig.app
            .raise_receive_mark(5)
            .expect_err("not higher")
            .rule(),
        "SR-70"
    );

    // A clock that is not behind changes nothing; one that is behind is
    // confirmed once the app says so.
    let before = rig.app.signer_status().expect("status");
    let saves = rig.store.saves();
    rig.app.correct_clock().expect("nothing to correct");
    assert_eq!(rig.app.signer_status().expect("status"), before);
    // Nothing changed, so nothing was stored.
    assert_eq!(rig.store.saves(), saves);
    rig.app
        .enroll_destination(address(5), "Exchange".to_string())
        .expect("enrolled");
    rig.store.set_now(NOW - 1_000);
    rig.app.correct_clock().expect("corrected");
    let status = rig.app.signer_status().expect("status");
    assert_eq!(status.latest_time_seen, NOW - 1_000);
    assert_eq!(status.allowlist[0].enrolled_at, NOW - 1_000);
}

#[test]
fn the_receive_mark_of_an_approver_only_key_app_goes_up_through_the_store() {
    let store = MemStore::new();
    let app = open_new(store.clone());
    app.create_approver_only().expect("identity");
    let (_, s1) = fixed_key(1, 1);
    let (_, s2) = fixed_key(2, 2);
    let (_, s3) = fixed_key(3, 3);
    let wallet =
        openbmt_core::wallet::WalletDescription::new(NET, [s1, s2, s3.clone()]).expect("a wallet");
    accept_with_no_key_2(&app, &wallet);
    assert_eq!(app.receive_mark().expect("mark"), 0);
    let saves = store.saves();
    app.raise_receive_mark(12).expect("raised");
    assert_eq!(store.saves(), saves + 1);
    assert_eq!(app.receive_mark().expect("mark"), 12);
    assert_eq!(
        app.raise_receive_mark(12).expect_err("not higher").rule(),
        "SR-70"
    );
    // The signer's own calls are refused: key 2 is not here.
    assert_eq!(
        app.raise_change_mark(5).expect_err("no signer").rule(),
        "AS-36"
    );
    let again = reopen(store);
    assert_eq!(again.receive_mark().expect("mark"), 12);
    assert_eq!(
        again.state().expect("state"),
        SetupState::Ready { holds_key2: false }
    );
}

// ---- a panic stays inside ------------------------------------------------------

impl KeyApp {
    /// Test only: panics inside the lock, the way a bug would.
    pub fn debug_panic(&self) -> Result<(), KeyError> {
        self.run(|_| -> Result<(), KeyError> { panic!("a test panic") })
    }
}

#[test]
fn a_panic_becomes_an_internal_error_and_poisons_the_object() {
    let rig = rig();
    let request = {
        let mut rig2 = rig_again(&rig);
        session_request(&mut rig2)
    };
    let session = rig.app.open_session(request).expect("opens");
    let err = rig.app.debug_panic().expect_err("the panic is caught");
    assert!(matches!(err, KeyError::Internal { .. }));
    assert_eq!(err.rule(), "internal");
    assert!(err.text().contains("a test panic"));
    // Every later call, exported or not, returns the same kind of error
    // and touches nothing.
    let saves = rig.store.saves();
    for result in [
        rig.app.state().map(|_| ()),
        rig.app.signer_status().map(|_| ()),
        rig.app.raise_change_mark(9),
        rig.app.open_session(vec![1]).map(|_| ()),
    ] {
        let e = result.expect_err("poisoned");
        assert!(matches!(e, KeyError::Internal { .. }), "{e}");
    }
    assert!(matches!(
        session.lines().expect_err("poisoned"),
        KeyError::Internal { .. }
    ));
    assert!(matches!(
        session.finish().expect_err("poisoned"),
        KeyError::Internal { .. }
    ));
    assert_eq!(rig.store.saves(), saves);
    // Dropping the handle after a poisoning does not panic.
    drop(session);
    // A new object from the stored bytes works.
    let again = reopen(rig.store.clone());
    assert!(again.state().is_ok());
}

fn rig_again(rig: &Rig) -> Rig {
    let approver = openbmt_core::identity::DeviceId::from_bytes(
        &rig.app.device_root().expect("root").try_into().expect("32"),
    )
    .expect("a root");
    Rig {
        app: Arc::clone(&rig.app),
        store: Arc::clone(&rig.store),
        owner: openbmt_owner::Owner::new(
            &OWNER_SECRET,
            rig.wallet.clone(),
            approver,
            policy(),
            openbmt_owner::Options::default(),
        )
        .expect("an owner"),
        wallet: rig.wallet.clone(),
        key1: rig.key1.clone(),
        key1_account: rig.key1_account,
        view: rig.view.clone(),
    }
}

/// The code of `text` with comments dropped, one entry per line.
fn code_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(n, l)| (n + 1, l.split("//").next().unwrap_or("")))
}

#[test]
fn the_only_unsafe_is_the_jni_glue_and_it_is_counted() {
    // The crate denies `unsafe`; `jni.rs` allows it for itself. No other
    // file may hold the word as code. The glue's own count is pinned: every
    // block is one JNI call or one read of the function table, and adding one
    // means changing the numbers below on purpose.
    let dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("src");
    let mut dirs = vec![dir];
    let mut seen = 0;
    while let Some(d) = dirs.pop() {
        for entry in std::fs::read_dir(d).expect("a directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            seen += 1;
            let text = std::fs::read_to_string(&path).expect("readable");
            if path.ends_with("src/jni.rs") {
                // The glue, without its test module.
                let glue = text.split("\n#[cfg(test)]").next().expect("a first part");
                let blocks = code_lines(glue)
                    .filter(|(_, l)| l.contains("unsafe {"))
                    .count();
                let impls = code_lines(glue)
                    .filter(|(_, l)| l.contains("unsafe impl"))
                    .count();
                assert_eq!(blocks, 14, "unsafe blocks in jni.rs");
                assert_eq!(impls, 2, "unsafe impls in jni.rs");
                assert_eq!(
                    code_lines(glue)
                        .filter(|(_, l)| l.contains("unsafe"))
                        .count(),
                    blocks + impls + 1,
                    "only blocks, impls and the allow of the file"
                );
                continue;
            }
            // This file spells the word in its own checks.
            if path.ends_with("tests/screen.rs") {
                continue;
            }
            for (n, line) in code_lines(&text) {
                if line.contains("unsafe") && !line.contains("deny(unsafe_code)") {
                    panic!("unsafe at {}:{n}", path.display());
                }
            }
        }
    }
    assert!(seen >= 12);
}
