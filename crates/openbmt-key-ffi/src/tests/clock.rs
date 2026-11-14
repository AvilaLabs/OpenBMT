//! A clock that fails (it reads 0) or reads before the floor is refused at
//! every step that needs the time (L3).

use super::kit::*;
use crate::app::CLOCK_FLOOR_SECS;
use crate::KeyError;

/// 2026-11-01T00:00:00Z.
#[test]
fn the_floor_is_the_first_of_november_2026() {
    assert_eq!(CLOCK_FLOOR_SECS, 1_793_491_200);
    let now = NOW;
    assert!(now >= CLOCK_FLOOR_SECS);
}

fn refused(e: &KeyError) -> bool {
    matches!(e, KeyError::State { .. }) && e.rule() == "clock"
}

#[test]
fn a_clock_of_zero_or_below_the_floor_enrols_nothing_and_corrects_nothing() {
    let rig = rig();
    for bad in [0, 1, CLOCK_FLOOR_SECS - 1] {
        rig.store.set_now(bad);
        let saves = rig.store.saves();
        let err = rig
            .app
            .enroll_destination(address(5), "Exchange".to_string())
            .expect_err("no enrolment");
        assert!(refused(&err), "{bad}: {err:?}");
        let err = rig.app.correct_clock().expect_err("no correction");
        assert!(refused(&err), "{bad}: {err:?}");
        assert_eq!(rig.store.saves(), saves, "{bad}: nothing stored");
    }
    // At the floor it works.
    rig.store.set_now(CLOCK_FLOOR_SECS);
    rig.app
        .enroll_destination(address(5), "Exchange".to_string())
        .expect("enrolled");
    let status = rig.app.signer_status().expect("status");
    assert_eq!(status.allowlist[0].enrolled_at, CLOCK_FLOOR_SECS);
}

#[test]
fn a_clock_of_zero_or_below_the_floor_opens_no_session_and_makes_no_invitation() {
    let mut rig = rig();
    let request = session_request(&mut rig);
    for bad in [0, CLOCK_FLOOR_SECS - 1] {
        rig.store.set_now(bad);
        let saves = rig.store.saves();
        let err = rig
            .app
            .open_session(request.clone())
            .err()
            .expect("refused");
        assert!(refused(&err), "{bad}: {err:?}");
        let err = rig.app.make_invitation().expect_err("refused");
        assert!(refused(&err), "{bad}: {err:?}");
        assert_eq!(rig.store.saves(), saves);
    }
    // No session was left open by the refusal.
    rig.store.set_now(NOW);
    rig.app.open_session(request).expect("opens");
}
