//! The owner phone's side of the switch from the starter wallet
//! (`docs/START-SIMPLE-RULES.md` SS-41, SS-42; the counter's side is
//! `docs/COUNTER-RULES.md` section 26).
//!
//! The owner makes one switch message for each counter that was paired with
//! the starter wallet ([`OwnShop::make_switch`](crate::own_shop::OwnShop)),
//! reads what each counter's records say of it (CR-262) and shows each
//! counter as switched, waiting or refused. The one move of everything in
//! the starter wallet (SS-42) waits until every counter in force is
//! switched: [`move_ready`] is that check, a pure function of the statuses.
//! The same check lets the owner's pinned wallet become the new wallet
//! ([`Repin`], `OwnShop::repin`); the rules name no moment for that
//! (`WORDS_NEEDED_RULE_SS_REPIN` under SS-41), so the caller chooses it.

use std::fmt;

use openbmt_core::identity::DeviceId;
use openbmt_core::say::{Tone, Who, Words};

/// What a counter's records say of the switch to one new wallet (SS-41).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SwitchStatus {
    /// CR-262: the counter's record for the new wallet says it switched.
    Switched,
    /// The counter has no record for the new wallet yet: the message may be
    /// on its way, or the manager has not answered.
    Waiting,
    /// CR-262: the counter's record for the new wallet says it did not
    /// switch. It keeps paying the starter wallet.
    Refused,
}

impl SwitchStatus {
    /// The byte the app is given.
    pub fn code(self) -> u8 {
        match self {
            Self::Switched => 0,
            Self::Waiting => 1,
            Self::Refused => 2,
        }
    }

    /// The status of a byte the app gave back, if it is one.
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Switched),
            1 => Some(Self::Waiting),
            2 => Some(Self::Refused),
            _ => None,
        }
    }
}

/// One record of CR-262 a counter holds: the switched byte and the hash of
/// the new wallet it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldSwitch {
    /// Whether the counter switched.
    pub switched: bool,
    /// The checksum of the new wallet's pinned frame.
    pub wallet: [u8; 32],
}

/// SS-41: the status of a counter whose records hold `held`, for the new
/// wallet whose hash is `new_wallet`. A record for another wallet says
/// nothing about this one. When the counter holds both bytes for the wallet
/// (it was refused, then asked again) the later record in `held` counts.
pub fn switch_status(held: &[HeldSwitch], new_wallet: &[u8; 32]) -> SwitchStatus {
    match held.iter().rev().find(|h| h.wallet == *new_wallet) {
        Some(h) if h.switched => SwitchStatus::Switched,
        Some(_) => SwitchStatus::Refused,
        None => SwitchStatus::Waiting,
    }
}

/// SS-42: why the move does not start yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveNotReady {
    /// Counters with no record for the new wallet yet.
    pub waiting: Vec<DeviceId>,
    /// Counters whose record says they did not switch.
    pub refused: Vec<DeviceId>,
}

impl fmt::Display for MoveNotReady {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SS-42: {} counters have not switched and {} refused",
            self.waiting.len(),
            self.refused.len()
        )
    }
}

impl std::error::Error for MoveNotReady {}

/// SS-42: the move of everything in the starter wallet may be built only
/// when every counter in the settings in force has the record of CR-262
/// with the switched byte for the new wallet. `counters` is each counter
/// of the settings in force and its status. With no counter there is
/// nothing to wait for.
pub fn move_ready(counters: &[(DeviceId, SwitchStatus)]) -> Result<(), MoveNotReady> {
    let pick = |wanted: SwitchStatus| -> Vec<DeviceId> {
        counters
            .iter()
            .filter(|(_, s)| *s == wanted)
            .map(|(c, _)| *c)
            .collect()
    };
    let waiting = pick(SwitchStatus::Waiting);
    let refused = pick(SwitchStatus::Refused);
    if waiting.is_empty() && refused.is_empty() {
        Ok(())
    } else {
        Err(MoveNotReady { waiting, refused })
    }
}

/// SS-41: what a call to re-pin the owner's own wallet did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repin {
    /// Every counter had switched: the pinned wallet is now the new wallet
    /// and the state was stored for it.
    Done,
    /// The new wallet is the pinned wallet already (this call, or an earlier
    /// one, did it): nothing was changed or stored.
    AlreadyDone,
    /// A counter has not switched: nothing was changed or stored.
    NotReady(MoveNotReady),
}

/// The words of a state of this part, by the key of its rule.
fn said(key: &'static str, tone: Tone) -> Words {
    let (heading, what, next) = match key {
        "OA_SWITCH_SWITCHED" => ("Counter switched", "This counter's sales go to the new wallet now.", "Nothing more to do for this counter."),
        "OA_SWITCH_WAITING" => ("Waiting for the counter", "This counter has the new wallet, but a manager has not checked it yet. Its sales still go to the starter wallet.", "At the counter, open New wallet to check and compare the address with the new wallet's own screen."),
        "OA_SWITCH_REFUSED" => ("Counter said different", "The manager at this counter said its address differs from the new wallet's own screen, so it keeps paying into the starter wallet.", "Check the new wallet, then send the switch again. If the addresses truly differ, this phone may not be safe."),
        "OA_SWITCH_MOVE_NOT_READY" => ("Not every counter switched", "The starter wallet's money moves only once every counter pays into the new wallet.", "Switch the counters that still wait, then move it."),
        _ => (key, key, key),
    };
    Words::new(tone, heading, what, Who::Owner, next)
}

/// SS-41: the words of a counter's status.
pub fn status_words(status: SwitchStatus) -> Words {
    match status {
        SwitchStatus::Switched => said("OA_SWITCH_SWITCHED", Tone::Info),
        SwitchStatus::Waiting => said("OA_SWITCH_WAITING", Tone::Info),
        SwitchStatus::Refused => said("OA_SWITCH_REFUSED", Tone::Stop),
    }
}

/// SS-42: the words of a move that is not ready.
pub fn not_ready_words() -> Words {
    said("OA_SWITCH_MOVE_NOT_READY", Tone::Stop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbmt_core::identity::Identity;

    fn id(n: u8) -> DeviceId {
        Identity::from_secret_bytes(&[n; 32]).unwrap().device_id()
    }

    #[test]
    fn ss_41_a_counter_is_switched_waiting_or_refused_by_its_record_for_the_new_wallet() {
        let new = [7u8; 32];
        let other = [8u8; 32];
        assert_eq!(switch_status(&[], &new), SwitchStatus::Waiting);
        // A record for another wallet says nothing about this one.
        let elsewhere = HeldSwitch {
            switched: true,
            wallet: other,
        };
        assert_eq!(switch_status(&[elsewhere], &new), SwitchStatus::Waiting);
        let yes = HeldSwitch {
            switched: true,
            wallet: new,
        };
        let no = HeldSwitch {
            switched: false,
            wallet: new,
        };
        assert_eq!(
            switch_status(&[elsewhere, yes], &new),
            SwitchStatus::Switched
        );
        assert_eq!(switch_status(&[no], &new), SwitchStatus::Refused);
        // The later record counts.
        assert_eq!(switch_status(&[no, yes], &new), SwitchStatus::Switched);
        assert_eq!(switch_status(&[yes, no], &new), SwitchStatus::Refused);
    }

    #[test]
    fn ss_42_the_move_is_ready_only_when_every_counter_is_switched() {
        use SwitchStatus::*;
        assert_eq!(move_ready(&[]), Ok(()));
        assert_eq!(move_ready(&[(id(1), Switched), (id(2), Switched)]), Ok(()));
        let e = move_ready(&[(id(1), Switched), (id(2), Waiting), (id(3), Refused)]).unwrap_err();
        assert_eq!(e.waiting, vec![id(2)]);
        assert_eq!(e.refused, vec![id(3)]);
        assert!(move_ready(&[(id(1), Waiting)]).is_err());
        assert!(move_ready(&[(id(1), Refused)]).is_err());
    }

    #[test]
    fn ss_41_the_status_codes_round_trip_and_each_has_its_words() {
        for s in [
            SwitchStatus::Switched,
            SwitchStatus::Waiting,
            SwitchStatus::Refused,
        ] {
            assert_eq!(SwitchStatus::from_code(s.code()), Some(s));
            let w = status_words(s);
            assert!(!w.heading.starts_with("WORDS_NEEDED_") && w.heading != w.what);
            assert!(w.heading.len() < 28 && w.what != w.next);
        }
        assert_eq!(SwitchStatus::from_code(3), None);
        assert_eq!(not_ready_words().heading, "Not every counter switched");
    }
}
