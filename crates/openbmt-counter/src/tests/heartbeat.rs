//! Section 6: the owner's heartbeat (CR-50 to CR-56).

use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use openbmt_core::heartbeat::{HeartbeatError, SignedHeartbeat, WatcherHeartbeat};
use openbmt_core::policy::{Heartbeat, OffReason};

use super::kit::*;
use crate::heartbeat::{MAX_AHEAD_SECS, TIP_SLACK_BLOCKS};
use crate::{
    judge_heartbeat, Facts, HeartbeatRefused, HeartbeatView, NotVerified, Screen, Slot, State, Why,
};

/// The hash the test chain holds at its tip, height 100.
const TIP: [u8; 32] = [0x55; 32];

fn chain() -> TestChain {
    let mut c = TestChain::new(100);
    c.add(BlockHash::from_byte_array(TIP), 100);
    c
}

fn hb(seq: u64, time: u64) -> WatcherHeartbeat {
    WatcherHeartbeat {
        counter: id(3).device_id(),
        seq,
        time,
        tip_height: 100,
        tip_hash: TIP,
        healthy: true,
    }
}

fn from_owner(h: WatcherHeartbeat) -> SignedHeartbeat {
    h.sign(&id(1))
}

fn world() -> World {
    let mut w = World::new();
    w.chain.add(BlockHash::from_byte_array(TIP), 100);
    w
}

impl World {
    fn take(
        &mut self,
        signed: &SignedHeartbeat,
    ) -> Result<HeartbeatView, HeartbeatRefused<&'static str>> {
        self.c.heartbeat(signed, &self.chain, self.now, &mut self.m)
    }

    /// Judge with the heartbeat the counter itself holds.
    fn judge_own(&mut self, index: u32) -> Screen {
        let facts = Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: self.c.heartbeat_for_policy(self.now),
            remote_order: false,
            now: self.now,
        };
        self.c.judge(index, &facts, &mut self.m).unwrap()
    }

    /// A paid sale seen by three groups, past the conflict watch.
    fn seen_paid(&mut self) -> u32 {
        let o = self.open_five();
        let t = self.pay(&o, 5_000);
        self.see(&t, 3);
        self.tick(10);
        o.index
    }
}

// CR-50.

#[test]
fn cr_50_instant_needs_a_heartbeat_the_counter_verified() {
    let mut w = world();
    let index = w.seen_paid();
    let screen = w.judge_own(index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::InstantOff(OffReason::NoWatcher));
    assert_eq!(
        screen.why_text(),
        "Waiting for a confirmation: accepting at once is off: no independent watcher."
    );
    assert_eq!(
        w.c.heartbeat_for_policy(w.now),
        Heartbeat {
            verified: false,
            age_secs: 0
        }
    );

    let signed = from_owner(hb(1, w.now));
    assert_eq!(
        w.take(&signed),
        Ok(HeartbeatView::Verified { sent_at: w.now })
    );
    let screen = w.judge_own(index);
    assert_eq!(screen.state, State::PaidInstant);
}

#[test]
fn cr_50_only_the_owner_root_signs_a_heartbeat() {
    let mut w = world();
    for who in [2u8, 3, 9] {
        let signed = hb(1, w.now).sign(&id(who));
        let err = w.take(&signed).unwrap_err();
        assert_eq!(
            err,
            HeartbeatRefused::Refused(HeartbeatError::OwnerSignatureInvalid),
            "signer {who}"
        );
        assert_eq!(err.rule(), "CR-50");
    }
    assert_eq!(w.c.heartbeat_view(), None);
    assert_eq!(w.c.desk().last_heartbeat_seq(), 0);
    assert!(w.m.log.is_empty(), "nothing was written: {:?}", w.m.log);
}

#[test]
fn cr_50_a_heartbeat_for_another_counter_is_refused() {
    let mut w = world();
    let mut h = hb(1, w.now);
    h.counter = id(4).device_id();
    let err = w.take(&from_owner(h)).unwrap_err();
    assert_eq!(err, HeartbeatRefused::Refused(HeartbeatError::WrongCounter));
    assert_eq!(w.c.heartbeat_view(), None);
}

#[test]
fn cr_50_the_number_must_be_above_the_last_one_taken() {
    let mut w = world();
    let signed = from_owner(hb(5, w.now));
    assert!(w.take(&signed).is_ok());
    for old in [5u64, 4, 1] {
        let err = w.take(&from_owner(hb(old, w.now))).unwrap_err();
        assert_eq!(
            err,
            HeartbeatRefused::Refused(HeartbeatError::Stale { last: 5, got: old })
        );
    }
    assert!(w.take(&from_owner(hb(6, w.now))).is_ok());
    assert_eq!(w.c.desk().last_heartbeat_seq(), 6);
}

#[test]
fn cr_50_the_refusal_reads_plainly() {
    let mut w = world();
    let err = w.take(&hb(1, w.now).sign(&id(2))).unwrap_err();
    assert_eq!(
        err.to_string(),
        "CR-50: heartbeat is not signed by the owner"
    );
}

// CR-51.

#[test]
fn cr_51_the_age_is_the_counters_time_minus_the_heartbeats() {
    let mut w = world();
    let sent = w.now - 40;
    w.take(&from_owner(hb(1, sent))).unwrap();
    for later in [0u64, 1, 40, 600] {
        assert_eq!(
            w.c.heartbeat_for_policy(w.now + later),
            Heartbeat {
                verified: true,
                age_secs: 40 + later
            }
        );
    }
    // a clock that went back gives age zero, not a huge number
    assert_eq!(
        w.c.heartbeat_for_policy(sent - 5),
        Heartbeat {
            verified: true,
            age_secs: 0
        }
    );
}

#[test]
fn cr_51_an_old_heartbeat_turns_instant_off() {
    let mut w = world();
    let index = w.seen_paid();
    w.take(&from_owner(hb(1, w.now))).unwrap();
    assert_eq!(w.judge_own(index).state, State::PaidInstant);

    // The signed setting is 600 seconds by default; a second sale, seen
    // later, finds the heartbeat too old.
    let max = u64::from(settings().heartbeat_max_age_secs);
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(max);
    w.see(&t, 3);
    w.tick(10);
    let screen = w.judge_own(o.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::InstantOff(OffReason::NoWatcher));
}

#[test]
fn cr_51_a_heartbeat_ahead_of_the_clock_by_more_than_120_seconds_is_refused() {
    let mut w = world();
    let err = w.take(&from_owner(hb(1, w.now + 121))).unwrap_err();
    assert_eq!(err, HeartbeatRefused::FromTheFuture { ahead_secs: 121 });
    assert_eq!(err.rule(), "CR-51");
    // refused whole: nothing stored, the number is not used up
    assert_eq!(w.c.heartbeat_view(), None);
    assert_eq!(w.c.desk().last_heartbeat_seq(), 0);
    assert!(w.m.log.is_empty());
    let ok = w.take(&from_owner(hb(1, w.now + MAX_AHEAD_SECS)));
    assert_eq!(
        ok,
        Ok(HeartbeatView::Verified {
            sent_at: w.now + 120
        })
    );
}

#[test]
fn cr_51_the_ahead_limit_is_judged_before_health_and_tip() {
    let c = chain();
    let mut h = hb(1, T0 + 500);
    h.healthy = false;
    h.tip_hash = [1; 32];
    assert_eq!(
        judge_heartbeat(&h, &c, T0),
        HeartbeatView::NotVerified(NotVerified::FromTheFuture { ahead_secs: 500 })
    );
    h.time = T0 + MAX_AHEAD_SECS;
    assert_eq!(
        judge_heartbeat(&h, &c, T0),
        HeartbeatView::NotVerified(NotVerified::OwnerUnhealthy)
    );
}

// CR-52.

#[test]
fn cr_52_an_unhealthy_heartbeat_is_taken_and_is_not_verified() {
    let mut w = world();
    let mut h = hb(1, w.now);
    h.healthy = false;
    assert_eq!(
        w.take(&from_owner(h)),
        Ok(HeartbeatView::NotVerified(NotVerified::OwnerUnhealthy))
    );
    // its number counts
    assert_eq!(w.c.desk().last_heartbeat_seq(), 1);
    assert!(w.take(&from_owner(h)).is_err());
    assert!(!w.c.heartbeat_for_policy(w.now).verified);
    let index = w.seen_paid();
    assert_eq!(w.judge_own(index).state, State::Waiting);
}

#[test]
fn cr_52_an_unhealthy_heartbeat_takes_a_verified_one_away_until_a_healthy_one_comes() {
    let mut w = world();
    let index = w.seen_paid();
    w.take(&from_owner(hb(1, w.now))).unwrap();
    assert!(w.c.heartbeat_for_policy(w.now).verified);
    let mut sick = hb(2, w.now);
    sick.healthy = false;
    w.take(&from_owner(sick)).unwrap();
    assert!(!w.c.heartbeat_for_policy(w.now).verified);
    assert_eq!(w.judge_own(index).state, State::Waiting);
    w.take(&from_owner(hb(3, w.now))).unwrap();
    assert!(w.c.heartbeat_for_policy(w.now).verified);
    assert_eq!(w.judge_own(index).state, State::PaidInstant);
}

// CR-53.

#[test]
fn cr_53_the_tip_must_be_one_the_counter_holds_within_two_blocks() {
    let mut c = chain();
    // the counter's tip is 100; hold 98 and 97 as well
    let h98 = BlockHash::from_byte_array([0x98; 32]);
    let h97 = BlockHash::from_byte_array([0x97; 32]);
    c.add(h98, 98);
    c.add(h97, 97);
    let at = |height: u32, hash: [u8; 32]| WatcherHeartbeat {
        tip_height: height,
        tip_hash: hash,
        ..hb(1, T0)
    };
    let verified = HeartbeatView::Verified { sent_at: T0 };
    assert_eq!(judge_heartbeat(&at(100, TIP), &c, T0), verified);
    assert_eq!(
        judge_heartbeat(&at(98, [0x98; 32]), &c, T0),
        verified,
        "two below the tip"
    );
    assert_eq!(TIP_SLACK_BLOCKS, 2);
    assert_eq!(
        judge_heartbeat(&at(97, [0x97; 32]), &c, T0),
        HeartbeatView::NotVerified(NotVerified::TipBehind { blocks: 3 })
    );
}

#[test]
fn cr_53_a_tip_the_counter_does_not_hold_yet_is_not_verified_and_not_an_alarm() {
    let c = chain();
    for ahead in [1u32, 2] {
        let h = WatcherHeartbeat {
            tip_height: 100 + ahead,
            tip_hash: [0x77; 32],
            ..hb(1, T0)
        };
        let view = judge_heartbeat(&h, &c, T0);
        assert_eq!(view, HeartbeatView::NotVerified(NotVerified::TipNotYetHeld));
        let HeartbeatView::NotVerified(why) = view else {
            panic!("verified");
        };
        assert!(!why.is_disagreement());
        assert_eq!(why.rule(), "CR-53");
    }
    let far = WatcherHeartbeat {
        tip_height: 103,
        tip_hash: [0x77; 32],
        ..hb(1, T0)
    };
    let view = judge_heartbeat(&far, &c, T0);
    assert_eq!(
        view,
        HeartbeatView::NotVerified(NotVerified::TipTooFarAhead)
    );
    assert!(!matches!(view, HeartbeatView::NotVerified(n) if n.is_disagreement()));
}

#[test]
fn cr_53_another_block_at_a_height_the_counter_holds_is_a_disagreement() {
    let c = chain();
    // another hash at the counter's own tip height
    let other = WatcherHeartbeat {
        tip_hash: [0x66; 32],
        ..hb(1, T0)
    };
    let view = judge_heartbeat(&other, &c, T0);
    assert_eq!(view, HeartbeatView::NotVerified(NotVerified::TipDisagrees));
    assert!(matches!(view, HeartbeatView::NotVerified(n) if n.is_disagreement()));
    // the counter's block, said to be at another height
    let moved = WatcherHeartbeat {
        tip_height: 99,
        ..hb(1, T0)
    };
    assert_eq!(
        judge_heartbeat(&moved, &c, T0),
        HeartbeatView::NotVerified(NotVerified::TipDisagrees)
    );
}

#[test]
fn cr_53_a_tip_that_is_not_yet_held_is_taken_and_counts_its_number() {
    let mut w = world();
    let mut h = hb(1, w.now);
    h.tip_height = 101;
    h.tip_hash = [0x77; 32];
    assert_eq!(
        w.take(&from_owner(h)),
        Ok(HeartbeatView::NotVerified(NotVerified::TipNotYetHeld))
    );
    assert_eq!(w.c.desk().last_heartbeat_seq(), 1);
    assert!(!w.c.heartbeat_for_policy(w.now).verified);
    // the counter catches up and the next heartbeat counts
    w.chain.tip = 101;
    w.chain.add(BlockHash::from_byte_array([0x77; 32]), 101);
    h.seq = 2;
    assert_eq!(
        w.take(&from_owner(h)),
        Ok(HeartbeatView::Verified { sent_at: w.now })
    );
}

// CR-54.

#[test]
fn cr_54_the_number_is_stored_before_the_heartbeat_counts() {
    let mut w = world();
    w.m.reset();
    w.m.fail_on = Some(1);
    let err = w.take(&from_owner(hb(1, w.now))).unwrap_err();
    assert!(matches!(err, HeartbeatRefused::Store(_)));
    assert_eq!(err.rule(), "CR-54");
    assert_eq!(w.m.log, ["FAILED Desk"]);
    assert_eq!(w.c.heartbeat_view(), None);
    assert!(!w.c.heartbeat_for_policy(w.now).verified);
    assert_eq!(w.c.desk().last_heartbeat_seq(), 0);

    // the same heartbeat is taken once the store works: its number was not
    // used up
    w.m.reset();
    assert!(w.take(&from_owner(hb(1, w.now))).is_ok());
    assert_eq!(w.m.log, ["Desk"]);
    let stored = crate::Desk::from_bytes(&w.m.slots[&Slot::Desk]).unwrap();
    assert_eq!(stored.last_heartbeat_seq(), 1);
}

#[test]
fn cr_54_a_failed_store_leaves_an_earlier_heartbeat_as_it_was() {
    let mut w = world();
    w.take(&from_owner(hb(1, w.now))).unwrap();
    w.m.reset();
    w.m.fail_all = true;
    let mut sick = hb(2, w.now);
    sick.healthy = false;
    assert!(w.take(&from_owner(sick)).is_err());
    assert!(w.c.heartbeat_for_policy(w.now).verified);
    assert_eq!(w.c.desk().last_heartbeat_seq(), 1);
}

#[test]
fn cr_54_after_a_restart_there_is_no_verified_heartbeat_and_replays_are_refused() {
    let mut w = world();
    w.take(&from_owner(hb(7, w.now))).unwrap();
    assert!(w.c.heartbeat_for_policy(w.now).verified);
    w.restart();
    assert_eq!(w.c.heartbeat_view(), None);
    assert_eq!(
        w.c.heartbeat_for_policy(w.now),
        Heartbeat {
            verified: false,
            age_secs: 0
        }
    );
    assert_eq!(w.c.desk().last_heartbeat_seq(), 7);
    let err = w.take(&from_owner(hb(7, w.now))).unwrap_err();
    assert_eq!(
        err,
        HeartbeatRefused::Refused(HeartbeatError::Stale { last: 7, got: 7 })
    );
    assert!(w.take(&from_owner(hb(8, w.now))).is_ok());
    assert!(w.c.heartbeat_for_policy(w.now).verified);
}

#[test]
fn cr_54_a_sale_that_waited_for_the_heartbeat_is_accepted_after_one_comes() {
    let mut w = world();
    let index = w.seen_paid();
    assert_eq!(w.judge_own(index).state, State::Waiting);
    w.take(&from_owner(hb(1, w.now))).unwrap();
    assert_eq!(w.judge_own(index).state, State::PaidInstant);
    w.restart();
    // accepted at once for good (CR-13): a restart does not take it back
    assert_eq!(w.judge_own(index).state, State::PaidInstant);
}

// CR-55.

#[test]
fn cr_55_the_boxs_paid_notice_is_not_a_heartbeat() {
    let mut w = world();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see_by_box(&t);
    w.tick(10);
    let screen = w.judge_own(o.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::InstantOff(OffReason::NoWatcher));
    assert_eq!(w.c.heartbeat_view(), None);
}

#[test]
fn cr_55_a_heartbeat_signed_by_the_approver_does_not_count() {
    let mut w = world();
    let index = w.seen_paid();
    let by_approver = hb(1, w.now).sign(&id(2));
    assert!(w.take(&by_approver).is_err());
    assert_eq!(w.judge_own(index).state, State::Waiting);
    // an owner-signed one with the approver's signature swapped in
    let mut mixed = from_owner(hb(1, w.now));
    mixed.owner_sig = by_approver.owner_sig;
    assert!(w.take(&mixed).is_err());
    assert_eq!(w.judge_own(index).state, State::Waiting);
}

// CR-186: the watch key of the settings in force.

fn watch() -> openbmt_core::identity::Identity {
    id(30)
}

fn naming_the_watch_key(w: &mut World) {
    let mut s = w.c.settings().clone();
    s.watch_key = Some(watch().device_id());
    w.c.set_settings(s);
}

#[test]
fn cr_186_a_heartbeat_signed_by_the_named_watch_key_counts() {
    let mut w = world();
    let index = w.seen_paid();
    let signed = hb(1, w.now).sign_watch(&watch());
    // not named yet: refused, and nothing is written
    let written = w.m.log.len();
    let err = w.take(&signed).unwrap_err();
    assert_eq!(
        err,
        HeartbeatRefused::Refused(HeartbeatError::OwnerSignatureInvalid)
    );
    assert_eq!(w.m.log.len(), written);
    naming_the_watch_key(&mut w);
    assert_eq!(
        w.take(&signed),
        Ok(HeartbeatView::Verified { sent_at: w.now })
    );
    assert_eq!(w.judge_own(index).state, State::PaidInstant);
    // The number line is shared with the owner root's heartbeats.
    let err = w.take(&from_owner(hb(1, w.now))).unwrap_err();
    assert!(matches!(
        err,
        HeartbeatRefused::Refused(HeartbeatError::Stale { last: 1, got: 1 })
    ));
    assert_eq!(
        w.take(&from_owner(hb(2, w.now))),
        Ok(HeartbeatView::Verified { sent_at: w.now })
    );
}

#[test]
fn cr_186_removing_the_watch_key_ends_its_heartbeats() {
    let mut w = world();
    naming_the_watch_key(&mut w);
    assert!(w.take(&hb(1, w.now).sign_watch(&watch())).is_ok());
    let mut s = w.c.settings().clone();
    s.watch_key = None;
    w.c.set_settings(s);
    let err = w.take(&hb(2, w.now).sign_watch(&watch())).unwrap_err();
    assert_eq!(
        err,
        HeartbeatRefused::Refused(HeartbeatError::OwnerSignatureInvalid)
    );
}

#[test]
fn cr_186_another_key_and_another_tag_do_not_count() {
    let mut w = world();
    naming_the_watch_key(&mut w);
    // another key under the watch tag
    assert!(w.take(&hb(1, w.now).sign_watch(&id(31))).is_err());
    // the named key under the owner's tag
    let wrong_tag = hb(1, w.now).sign(&watch());
    assert!(w.take(&wrong_tag).is_err());
    assert_eq!(w.c.heartbeat_view(), None);
    assert_eq!(w.c.desk().last_heartbeat_seq(), 0);
}
