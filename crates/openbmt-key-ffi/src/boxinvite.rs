//! "Pair a Box": the invitation the Key app makes and shows (SR-80, SR-81).
//!
//! The invitation is `Approver::make_invitation`, stored by the Key app
//! before it is shown, with an end 30 minutes after the app's clock
//! (BX-10, BX-11). The Key app's lines for the Box's claim come with the
//! session (`BoxClaimDetail`); what they check is AS-81.

use std::sync::Arc;

use openbmt_approver::{InvitationView as Made, StateError};
use openbmt_core::boxpair::INVITATION_RANDOM_LEN;

use crate::app::{save_ready, KeyApp, ReadyState};
use crate::codec::wire_record;
use crate::error::{internal, refused, store_failed, KeyError};
use crate::keys::random_bytes;

/// An invitation the Key app holds (SR-80).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitationView {
    /// The 30 characters in six groups of five, separated by hyphens
    /// (BX-10).
    pub text: String,
    /// When it runs out, in seconds since 1970 on this phone's clock.
    pub expires_at: u64,
    /// The seconds left on this phone's clock when the call was made.
    pub secs_left: u64,
}

wire_record!(InvitationView {
    text,
    expires_at,
    secs_left
});

fn view(made: Made, now: u64) -> InvitationView {
    InvitationView {
        text: made.text,
        expires_at: made.expires_at,
        secs_left: made.expires_at.saturating_sub(now),
    }
}

impl KeyApp {
    /// SR-80, BX-10, BX-11: make an invitation from the platform's random
    /// source, store it with its end, and only then return it. It replaces
    /// the invitation that was open.
    pub fn make_invitation(&self) -> Result<InvitationView, KeyError> {
        self.run(|inner| {
            inner.no_session()?;
            let now = inner.store.now();
            let store = Arc::clone(&inner.store);
            let mut bytes = random_bytes(&store, INVITATION_RANDOM_LEN)?;
            let mut random = [0u8; INVITATION_RANDOM_LEN];
            let given = bytes.get(..INVITATION_RANDOM_LEN);
            match given {
                Some(g) => random.copy_from_slice(g),
                None => return Err(internal("random: short")),
            }
            bytes.fill(0);
            let ReadyState {
                material,
                wallet,
                approver,
            } = inner.ready()?;
            let made = approver
                .make_invitation(&random, now, |snapshot| {
                    save_ready(&store, material, wallet, snapshot)
                })
                .map_err(|e| match e {
                    StateError::Store(why) => store_failed(&why),
                    other => refused(other),
                });
            random.fill(0);
            Ok(view(made?, now))
        })
    }

    /// SR-80: the invitation that is open and has not run out, or `None`.
    pub fn open_invitation(&self) -> Result<Option<InvitationView>, KeyError> {
        self.run(|inner| {
            let now = inner.store.now();
            let ready = inner.ready()?;
            Ok(ready
                .approver
                .open_invitation(now)
                .map(|made| view(made, now)))
        })
    }

    /// SR-80: forget the invitation that is open. The state is stored first.
    pub fn cancel_invitation(&self) -> Result<(), KeyError> {
        self.run(|inner| {
            inner.no_session()?;
            let store = Arc::clone(&inner.store);
            let ReadyState {
                material,
                wallet,
                approver,
            } = inner.ready()?;
            approver
                .cancel_invitation(|snapshot| save_ready(&store, material, wallet, snapshot))
                .map_err(|e| match e {
                    StateError::Store(why) => store_failed(&why),
                    other => refused(other),
                })
        })
    }
}
