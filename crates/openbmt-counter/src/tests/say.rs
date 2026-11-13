//! SC-01 to SC-12 for the counter's words.

use openbmt_core::policy::{OffReason, WaitReason};
use openbmt_core::say::{check, check_distinct, Quoted, Tone, Who};
use openbmt_watch::Problem;

use crate::desk::Role;
use crate::say::{
    all_words, next_name, refund_state_name, sample_screens, state_name, why_name,
    ALL_REFUND_STATES, ALL_STATES,
};
use crate::screen::{Color, Next, Why};
use crate::{Actor, RefundState, RefundView};

#[test]
fn sc_12_every_word_keeps_the_rules() {
    // CR-271: a placeholder is not a sentence and is not read as one, but
    // each stands for one state, so each differs from every other.
    let (placeholders, written): (Vec<_>, Vec<_>) = all_words()
        .into_iter()
        .partition(|(_, w)| w.heading.starts_with("WORDS_NEEDED_"));
    let mut seen = std::collections::BTreeSet::new();
    for (name, w) in &placeholders {
        assert!(
            seen.insert(w.heading.clone()),
            "{name} repeats a placeholder"
        );
        assert_eq!(w.heading, w.what, "{name}");
        assert_eq!(w.heading, w.next, "{name}");
    }
    assert_eq!(check_distinct(&written), Ok(()));
}

#[test]
fn sc_01_no_variant_is_missing() {
    let all = all_words();
    let has = |name: &str| all.iter().any(|(n, _)| n == name);
    let screens = sample_screens();
    // Every state, every reason and every next step is on some screen.
    for state in ALL_STATES {
        assert!(
            screens
                .iter()
                .any(|(_, s)| state_name(s.state) == state_name(state)),
            "{}",
            state_name(state)
        );
    }
    let whys = [
        "NothingArrived",
        "Part",
        "NotSeenEnough",
        "Wait",
        "InstantOff",
        "NotStored",
        "PauseNotSaved",
        "Instant",
        "ConfirmedAt",
        "FinalAt",
        "OverriddenBy",
        "Replaced",
        "Reopened",
        "CannotCheck",
        "WatcherNeedsAPerson",
        "StoredFormUnreadable",
        "EndedExpired",
        "EndedCancelled",
    ];
    for name in whys {
        assert!(
            screens.iter().any(|(_, s)| why_name(&s.why) == name),
            "{name}"
        );
    }
    let nexts = [
        "CustomerPays",
        "CustomerPaysRest",
        "KeepWatching",
        "WatchSeconds",
        "WaitForConfirmation",
        "FinalAt",
        "OwnerClears",
        "AskOwnerOrOtherPayment",
        "PersonLooks",
        "Nothing",
    ];
    for name in nexts {
        assert!(
            screens.iter().any(|(_, s)| next_name(&s.next) == name),
            "{name}"
        );
    }
    // Every refusal type, and each refund state, has words.
    for prefix in [
        "OpenRefused::",
        "JudgeError::",
        "RequoteRefused::",
        "CancelRefused::",
        "OverrideRefused::",
        "CommandRefused::",
        "HeartbeatRefused::",
        "NotVerified::",
        "HeartbeatView::",
        "SightingError::",
        "BlockError::",
        "LoadError::",
        "DestinationRefused::",
        "RefundRefused::",
        "WithdrawRefused::",
        "StaffRefused::",
        "TotalsRefused::",
    ] {
        assert!(all.iter().any(|(n, _)| n.starts_with(prefix)), "{prefix}");
    }
    for state in ALL_REFUND_STATES {
        assert!(has(&format!("RefundState::{}", refund_state_name(state))));
        assert!(has(&format!("RefundView::{}", refund_state_name(state))));
    }
    for state in ALL_STATES {
        assert!(has(&format!(
            "OverrideRefused::WrongState/{}",
            state_name(state)
        )));
    }
}

#[test]
fn sc_05_someone_is_named() {
    for (name, w) in all_words() {
        assert!(
            !(w.who == Who::Nobody && w.tone == Tone::Stop),
            "{name} stops and names nobody"
        );
    }
}

#[test]
fn sc_06_the_colour_is_the_states() {
    for (name, screen) in sample_screens() {
        let words = screen.words();
        let want = match screen.state.color() {
            Color::Blue => Tone::Info,
            Color::Green => Tone::Good,
            Color::Red => Tone::Stop,
            Color::Neutral => Tone::Ended,
        };
        assert_eq!(words.tone, want, "{name}");
        // The reason's own tone agrees with the state it is built for.
        assert_eq!(screen.why.words().tone, want, "{name}");
    }
}

#[test]
fn sc_07_waiting_and_instant_off_read_differently() {
    let over = Why::Wait(WaitReason::OverSaleLimit).words();
    let off = Why::InstantOff(OffReason::Disabled).words();
    let cannot = Why::InstantOff(OffReason::CheckUnhealthy).words();
    assert_ne!(over.what, off.what);
    assert_ne!(off.what, cannot.what);
    assert_ne!(over.what, cannot.what);
    // Every waiting case says nothing is to be done, or what to take instead.
    for reason in [
        WaitReason::ConflictSeen,
        WaitReason::RemoteOrder,
        WaitReason::Underpaid,
        WaitReason::OverSaleLimit,
        WaitReason::SliceFull,
        WaitReason::ConflictWatch { secs_left: 3 },
    ] {
        let w = Why::Wait(reason).words();
        assert_eq!(w.tone, Tone::Info);
        assert!(w.next.starts_with("Nothing to do."), "{}", w.next);
    }
}

#[test]
fn sc_06_red_states_say_do_not_hand_over() {
    for why in [Why::Replaced, Why::Reopened, Why::CannotCheck(None)] {
        let w = why.words();
        assert_eq!(w.tone, Tone::Stop);
        assert_eq!(w.heading, "Do not hand over");
        assert!(w.next.contains("another kind of payment"));
        assert!(w.next.contains("tell the owner"));
        assert!(w.next.contains("goods are gone"));
    }
    let replaced = Why::Replaced.words();
    let reopened = Why::Reopened.words();
    let cannot = Why::CannotCheck(None).words();
    assert_ne!(replaced.what, reopened.what);
    assert_ne!(replaced.what, cannot.what);
    assert_ne!(reopened.what, cannot.what);
}

#[test]
fn sc_03_a_cashier_is_not_shown_which_problem_the_check_has() {
    for problem in [
        Problem::TooFewPeerGroups,
        Problem::TooFewFilterPeers,
        Problem::TipIsOld,
        Problem::HeadersBehind,
        Problem::ClockMismatch,
    ] {
        let some = Why::CannotCheck(Some(problem)).words();
        let none = Why::CannotCheck(None).words();
        assert_eq!(some.heading, none.heading);
        assert_eq!(some.what, none.what);
        assert_eq!(some.next, none.next);
        assert!(some.detail.is_some());
        assert_eq!(check(&some), Ok(()));
    }
}

#[test]
fn sc_09_the_shortfall_is_said_once() {
    let w = Why::Part {
        received_sats: 4_000,
        needs_sats: 10_000,
    }
    .words();
    assert!(w.what.contains("6,000 sats"));
    assert!(!w.next.contains("sats"));
}

#[test]
fn sc_03_a_stale_price_has_its_time_in_at_only() {
    use crate::OpenRefused;
    let w = OpenRefused::<String>::StaleRate {
        fetched_at: 1_700_000_000,
        now: 1_700_009_000,
    }
    .words();
    assert_eq!(w.at, Some(1_700_000_000));
    assert!(!w.what.contains("second"));
    assert!(!w.next.contains("second"));
}

#[test]
fn sc_10_the_store_failures_say_which_step_was_not_done() {
    use crate::{
        CancelRefused, CommandRefused, HeartbeatRefused, JudgeError, OpenRefused, OverrideRefused,
        RefundRefused, RequoteRefused, StoreFault, WithdrawRefused,
    };
    let f = || StoreFault::Write("the disk is full".to_string());
    let eight = [
        OpenRefused::<String>::Store(f()).words(),
        JudgeError::<String>::Store(f()).words(),
        RequoteRefused::<String>::Store(f()).words(),
        CancelRefused::<String>::Store(f()).words(),
        OverrideRefused::<String>::Store(f()).words(),
        RefundRefused::<String>::Store(f()).words(),
        WithdrawRefused::<String>::Store(f()).words(),
        CommandRefused::<String>::Store(f()).words(),
        HeartbeatRefused::<String>::Store(f()).words(),
    ];
    for (i, a) in eight.iter().enumerate() {
        assert!(a
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("the disk is full")));
        assert!(!a.what.contains("disk"));
        assert!(a.next.contains("Details"));
        for b in eight.iter().skip(i + 1) {
            assert_ne!(a.what, b.what);
        }
    }
}

#[test]
fn sc_08_the_name_of_who_asked_is_quoted() {
    let view = RefundView {
        sale: 1,
        number: Some(2),
        asked: 5_000,
        value_minor: None,
        tip_minor: 0,
        state: RefundState::Requested,
        by: Actor {
            role: Role::Manager,
            name: "Ann\n\u{202e}Lee".to_string(),
        },
        at: 1_700_000_000,
    };
    let w = view.words();
    assert_eq!(w.quoted, Some(Quoted::new("The manager wrote", "Ann Lee")));
    assert_eq!(w.at, Some(1_700_000_000));
    assert!(!w.what.contains("Ann"));
}

#[test]
fn cr_74_a_requested_refund_says_the_counter_is_not_told() {
    let w = RefundState::Requested.words();
    assert!(w.what.contains("went to the owner"));
    assert!(w.what.contains("not told when it is paid"));
    assert!(w.next.contains("paid from the owner's side"));
}

#[test]
fn sc_07_the_same_fault_in_two_places_differs_by_step() {
    use crate::{OpenRefused, RefundRefused, RequoteRefused};
    let a = OpenRefused::<String>::AmountTooLarge.words();
    let b = RequoteRefused::<String>::AmountTooLarge.words();
    let c = RefundRefused::<String>::AmountTooLarge.words();
    assert_ne!(a.heading, b.heading);
    assert_ne!(b.heading, c.heading);
}

#[test]
fn sc_01_the_wait_next_is_never_used_for_stop() {
    for (name, screen) in sample_screens() {
        if matches!(
            screen.next,
            Next::WaitForConfirmation { .. } | Next::WatchSeconds { .. }
        ) {
            assert_ne!(screen.words().tone, Tone::Stop, "{name}");
        }
    }
}

#[test]
fn sc_12_the_flags_have_words() {
    let mut s = sample_screens().remove(0).1;
    assert!(s.flag_words().is_empty());
    s.stuck = true;
    s.late = true;
    let notes = s.flag_words();
    assert_eq!(notes.len(), 2);
    for n in notes {
        assert_eq!(check(&n), Ok(()));
    }
}
