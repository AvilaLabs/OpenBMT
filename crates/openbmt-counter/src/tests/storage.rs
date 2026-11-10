//! Section 5: storage (CR-40 to CR-42). Every multi-write step is stopped at
//! each of its writes in turn, the store is read back, and the state found is
//! on the safe side.

use openbmt_core::command::Action;
use openbmt_core::policy::PauseStatus;
use openbmt_core::records::{Chain, DeviceList, Record};

use super::kit::*;
use crate::{
    sale_payment_id, CommandRefused, Counter, Facts, JudgeError, LoadError, OpenRefused, Role,
    Slot, State, Why,
};

fn judge_with(w: &mut World, index: u32) -> Result<crate::Screen, JudgeError<&'static str>> {
    let facts = Facts {
        chain: &w.chain,
        health: w.health,
        heartbeat: heartbeat(),
        remote_order: false,
        now: w.now,
    };
    w.c.judge(index, &facts, &mut w.m)
}

/// A sale seen by three groups whose conflict watch has just run out: the
/// next judge accepts it at once. The sale is index 0.
fn ready_to_accept() -> World {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.tick(8);
    w
}

/// A sale accepted at once and then hit by a conflict: the next judge is a
/// cancel. The sale is index 0.
fn ready_to_cancel() -> World {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    w.c.sighting(&bytes(&cancel_of(&t)), peer(30), w.now, &mut w.m)
        .unwrap();
    w
}

/// A sale that waits (one group), judged once so that the next call writes
/// nothing of its own.
fn ready_to_override() -> World {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    w.judge(o.index);
    w
}

// CR-40.

#[test]
fn cr_40_everything_the_counter_remembers_is_in_the_store_and_reads_back() {
    let mut w = ready_to_cancel();
    let b = w.open_five();
    let t = w.pay(&b, 5_000);
    w.see(&t, 1);
    w.judge(0);
    w.judge(b.index);
    w.override_sale(b.index, Role::Cashier, "ann").unwrap();
    w.command(1, Action::ClearCancelAlarm).unwrap();
    for slot in [
        Slot::Batches,
        Slot::Exposure,
        Slot::Pause,
        Slot::Desk,
        Slot::Sale(0),
        Slot::Sale(1),
    ] {
        assert!(w.m.slots.contains_key(&slot), "{slot:?}");
    }
    // What is held is what is stored.
    assert_eq!(w.m.slots[&Slot::Batches], w.c.batches().to_bytes());
    assert_eq!(w.m.slots[&Slot::Desk], w.c.desk().to_bytes());
    assert_eq!(w.m.slots[&Slot::Sale(0)], w.c.sale(0).unwrap().to_bytes());
    assert_eq!(w.m.slots[&Slot::Sale(1)], w.c.sale(1).unwrap().to_bytes());
    // The records are a chain signed by the counter.
    let chain = w.m.stored().records;
    assert_eq!(chain.len() as usize, w.m.records.len());
    assert_eq!(chain.verify(&DeviceList::new([id(3).device_id()])), Ok(()));

    // A counter started from the store judges the same.
    let live: Vec<_> = [0, 1].into_iter().map(|i| w.judge(i)).collect();
    let alarm = w.c.alarm();
    let outstanding = w.c.exposure_outstanding();
    let pause = w.c.pause_status(w.now);
    w.restart();
    assert_eq!(w.c.alarm(), alarm);
    assert_eq!(w.c.exposure_outstanding(), outstanding);
    assert_eq!(w.c.pause_status(w.now), pause);
    for (i, before) in live.iter().enumerate() {
        let after = w.judge(i as u32);
        assert_eq!(after.state, before.state);
        assert_eq!(after.why, before.why);
        assert_eq!(after.received_sats, before.received_sats);
        assert_eq!(after.needs_sats, before.needs_sats);
        assert!(after.reports.is_empty());
    }
}

#[test]
fn cr_40_a_counter_that_never_ran_starts_from_an_empty_store() {
    let m = Mem::default();
    let c = m.reload();
    assert!(!c.alarm());
    assert_eq!(c.exposure_outstanding(), 0);
    assert_eq!(c.pause_status(T0), PauseStatus::Running);
    assert_eq!(c.unreadable().count(), 0);
}

#[test]
fn cr_40_a_failed_write_changes_nothing_in_memory() {
    let mut w = ready_to_override();
    let before = (
        w.c.desk().clone(),
        w.c.exposure_outstanding(),
        w.c.sale(0).unwrap().to_bytes(),
    );
    for n in 1..=3 {
        w.m.reset();
        w.m.fail_on = Some(n);
        assert!(w.override_sale(0, Role::Cashier, "ann").is_err());
        if n == 1 {
            let after = (
                w.c.desk().clone(),
                w.c.exposure_outstanding(),
                w.c.sale(0).unwrap().to_bytes(),
            );
            assert_eq!(before, after);
        }
    }
    // A failed batch write leaves the batches as they were.
    w.m.reset();
    w.m.fail_on = Some(1);
    let remaining = w.c.batches().remaining();
    assert!(w.c.add_batch(batch(100, 5), &mut w.m).is_err());
    assert_eq!(w.c.batches().remaining(), remaining);
}

// CR-41: opening.

#[test]
fn cr_41_open_stopped_at_each_of_its_four_writes_is_safe() {
    // Writes: the desk, the batches, the sale, the record.
    for n in 1..=4 {
        let mut w = World::new();
        w.m.fail_on = Some(n);
        let facts = w.open_facts();
        let refused =
            w.c.open(five_dollars(), usd(), rate(), facts, &mut w.m)
                .unwrap_err();
        assert_eq!(
            refused.rule(),
            if n == 2 { "CR-03" } else { "CR-40" },
            "write {n}"
        );
        w.m.reset();
        w.restart();
        // Only a sale whose sale write went through is there.
        assert_eq!(w.c.sale(0).is_some(), n == 4, "write {n}");
        let next = w.open_five();
        // An address counted and not used is wasted, never used twice.
        let expect = if n <= 2 { 0 } else { 1 };
        assert_eq!(next.index, expect, "write {n}");
        if n == 4 {
            // It is not shown to anyone and expires by itself.
            w.tick(crate::EXPIRY_SECS);
            assert_eq!(w.judge(0).state, State::Expired);
        }
    }
}

// CR-41: accepting at once.

#[test]
fn cr_41_accepting_at_once_stopped_at_each_of_its_three_writes_is_safe() {
    // Writes: the ledger, the record, the sale.
    for n in 1..=3 {
        let mut w = ready_to_accept();
        w.m.reset();
        w.m.fail_on = Some(n);
        let screen = w.judge(0);
        assert_eq!(screen.state, State::Waiting, "write {n}");
        assert!(screen.reports.is_empty(), "write {n}");
        w.m.reset();
        w.restart();
        // Exposure counted and not shown as paid.
        assert_eq!(
            w.c.exposure_outstanding(),
            if n == 1 { 0 } else { 5_000 },
            "write {n}"
        );
        assert_eq!(w.c.sale(0).unwrap().accepted(), None, "write {n}");
        // Carried on after the restart, without counting twice.
        let screen = w.judge(0);
        assert_eq!(screen.state, State::PaidInstant, "write {n}");
        assert_eq!(screen.reports, vec![crate::Report::PosPaid { index: 0 }]);
        assert_eq!(w.c.exposure_outstanding(), 5_000, "write {n}");
        assert!(w.judge(0).reports.is_empty());
    }
}

// CR-41: a cancel.

#[test]
fn cr_41_a_cancel_stopped_at_each_of_its_four_writes_is_safe() {
    // Writes: the pause, the desk, the record, the sale.
    for n in 1..=4 {
        // The same process asks again.
        let mut w = ready_to_cancel();
        w.m.reset();
        w.m.fail_on = Some(n);
        let first = judge_with(&mut w, 0);
        if n == 1 {
            // A pause that cannot be saved still takes effect, and the rest
            // of the cancel goes on.
            let screen = first.unwrap();
            assert_eq!(screen.state, State::DoNotHandOver);
            assert!(w.c.pause_unsaved());
            assert!(w.c.alarm());
            assert!(screen.reports.contains(&crate::Report::Alarm { index: 0 }));
        } else {
            assert!(matches!(first, Err(JudgeError::Store(_))), "write {n}");
        }
        w.m.reset();
        let screen = w.judge(0);
        assert_eq!(screen.state, State::DoNotHandOver, "write {n}");
        assert!(w.c.alarm(), "write {n}");
        assert!(
            matches!(w.c.pause_status(w.now), PauseStatus::PausedFor { .. }),
            "write {n}: counted once, not twice"
        );
        if n > 1 {
            assert!(screen.reports.contains(&crate::Report::Alarm { index: 0 }));
            assert!(screen
                .reports
                .contains(&crate::Report::PosUnpaid { index: 0 }));
        }
        assert!(w.judge(0).reports.is_empty());

        // The process stops and starts again after the failed write.
        let mut w = ready_to_cancel();
        w.m.reset();
        w.m.fail_on = Some(n);
        let _ = judge_with(&mut w, 0);
        w.m.reset();
        w.restart();
        let screen = w.judge(0);
        assert_eq!(screen.state, State::DoNotHandOver, "write {n}: never green");
        if n > 1 {
            assert!(w.c.alarm(), "write {n}");
        }
    }
}

#[test]
fn cr_41_a_cancel_whose_desk_write_was_lost_in_a_restart_may_count_twice_which_only_tightens() {
    let mut w = ready_to_cancel();
    w.m.reset();
    w.m.fail_on = Some(2); // the desk, after the pause
    assert!(judge_with(&mut w, 0).is_err());
    w.m.reset();
    w.restart();
    w.judge(0);
    assert!(w.c.alarm());
    assert_eq!(w.c.pause_status(w.now), PauseStatus::UntilOwnerReset);
}

// CR-41: an override.

#[test]
fn cr_41_an_override_stopped_at_each_of_its_four_writes_is_safe() {
    // Writes: the ledger, the desk, the record, the sale.
    for n in 1..=4 {
        let mut w = ready_to_override();
        w.m.reset();
        w.m.fail_on = Some(n);
        let err = w.override_sale(0, Role::Cashier, "ann").unwrap_err();
        assert_eq!(err.rule(), "CR-40", "write {n}");
        w.m.reset();
        w.restart();
        // Never shown as accepted; the ledger and the desk may be ahead.
        assert_eq!(w.judge(0).state, State::Waiting, "write {n}");
        assert_eq!(
            w.c.exposure_outstanding(),
            if n == 1 { 0 } else { 5_000 },
            "write {n}"
        );
        let counted = w.c.desk().override_total(Role::Cashier, "ann", w.now);
        assert_eq!(counted, if n <= 2 { 0 } else { 5_000 }, "write {n}");
        // Asking again works and does not count the ledger twice.
        w.override_sale(0, Role::Cashier, "ann").unwrap();
        assert_eq!(w.judge(0).state, State::Overridden, "write {n}");
        assert_eq!(w.c.exposure_outstanding(), 5_000, "write {n}");
    }
}

// CR-41: commands.

#[test]
fn cr_41_a_command_whose_number_could_not_be_stored_is_not_used() {
    let mut w = ready_to_cancel();
    w.judge(0);
    w.m.reset();
    w.m.fail_on = Some(1);
    let err = w.command(1, Action::ClearCancelAlarm).unwrap_err();
    assert!(matches!(err, CommandRefused::Store(_)));
    assert_eq!(err.rule(), "CR-40");
    assert!(w.c.alarm());
    w.m.reset();
    w.restart();
    assert!(w.c.alarm());
    w.command(1, Action::ClearCancelAlarm).unwrap();
    assert!(!w.c.alarm());
}

#[test]
fn cr_41_a_command_is_used_up_even_if_its_action_could_not_be_stored() {
    // A command that could be played again is worse than one that is lost.
    let mut w = ready_to_cancel();
    w.judge(0);
    w.m.reset();
    w.m.fail_on = Some(2); // the pause, after the number
    assert!(w.command(1, Action::ResetPause).is_err());
    w.m.reset();
    w.restart();
    assert_eq!(w.c.desk().last_command_seq(), 1);
    assert!(matches!(
        w.command(1, Action::ResetPause),
        Err(CommandRefused::Refused(_))
    ));
    assert!(
        matches!(w.c.pause_status(w.now), PauseStatus::PausedFor { .. }),
        "the pause is still on, the safe side"
    );
    // The owner sends another.
    w.command(2, Action::ResetPause).unwrap();
    assert_eq!(w.c.pause_status(w.now), PauseStatus::Running);

    let mut w = ready_to_cancel();
    w.judge(0);
    w.m.reset();
    w.m.fail_on = Some(2); // the ledger, after the number
    assert!(w
        .command(1, Action::ReleaseExposure(sale_payment_id(0)))
        .is_err());
    w.restart();
    assert_eq!(
        w.c.exposure_outstanding(),
        5_000,
        "still counted, the safe side"
    );
    assert!(w.command(1, Action::ResetPause).is_err());
    w.command(2, Action::ReleaseExposure(sale_payment_id(0)))
        .unwrap();
    assert_eq!(w.c.exposure_outstanding(), 0);
}

// CR-41: cancelling a sale.

#[test]
fn cr_41_cancelling_a_sale_stopped_at_each_of_its_two_writes_is_safe() {
    // Writes: the record, the sale.
    for n in 1..=2 {
        let mut w = World::new();
        let o = w.open_five();
        w.m.reset();
        w.m.fail_on = Some(n);
        assert!(w.c.cancel(o.index, w.now, &mut w.m).is_err());
        w.m.reset();
        w.restart();
        assert_eq!(w.judge(o.index).state, State::NotPaidYet, "write {n}");
        w.c.cancel(o.index, w.now, &mut w.m).unwrap();
        assert_eq!(w.judge(o.index).state, State::Cancelled);
    }
}

// CR-41: sightings and blocks.

#[test]
fn cr_41_a_sighting_or_block_that_could_not_be_stored_changes_nothing_and_can_be_offered_again() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.m.reset();
    w.m.fail_on = Some(1);
    let err =
        w.c.sighting(&bytes(&t), peer(20), w.now, &mut w.m)
            .unwrap_err();
    assert_eq!(err.rule(), "CR-40");
    assert_eq!(w.c.sale(0).unwrap().watch().transactions().count(), 0);
    w.m.reset();
    assert!(w.see(&t, 1).results.len() == 1);

    let block = w.mine_at(std::slice::from_ref(&t), 100);
    let wf = openbmt_watch::WellFormedBlock::parse(&bitcoin::consensus::encode::serialize(&block))
        .unwrap();
    w.m.reset();
    w.m.fail_on = Some(1);
    let err = w.c.block(&wf, &w.chain, w.now, &mut w.m).unwrap_err();
    assert_eq!(err.rule(), "CR-40");
    w.m.reset();
    let taken = w.c.block(&wf, &w.chain, w.now, &mut w.m).unwrap();
    assert_eq!(taken.results.len(), 1);
    assert_eq!(w.judge(0).state, State::Confirmed);
}

#[test]
fn cr_41_a_block_that_is_not_on_the_chain_is_refused_whole() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    let block = mined(9, &[t]);
    let wf = openbmt_watch::WellFormedBlock::parse(&bitcoin::consensus::encode::serialize(&block))
        .unwrap();
    let err = w.c.block(&wf, &w.chain, w.now, &mut w.m).unwrap_err();
    assert_eq!(err.rule(), "WR-01");
    assert_eq!(w.judge(o.index).state, State::NotPaidYet);
}

// CR-42.

#[test]
fn cr_42_a_sale_whose_stored_form_does_not_read_back_needs_a_person_never_paid() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 100);
    assert_eq!(w.judge(o.index).state, State::Confirmed);

    let good = w.m.slots[&Slot::Sale(0)].clone();
    let mut damaged: Vec<Vec<u8>> = Vec::new();
    // cut short
    damaged.push(good[..good.len() - 1].to_vec());
    // a byte too many
    let mut longer = good.clone();
    longer.push(0);
    damaged.push(longer);
    // another version
    let mut other = good.clone();
    other[4] = 2;
    damaged.push(other);
    // a flag that is neither 0 nor 1: the byte after the frozen need
    let mut flag = good.clone();
    let at = flag.len() / 2;
    flag[at] = 0xff;
    damaged.push(flag);

    for bytes in damaged {
        w.m.slots.insert(Slot::Sale(0), bytes);
        let mut c = w.m.reload();
        assert!(c.sale(0).is_none() || c.unreadable().count() == 0);
        if c.unreadable().count() == 1 {
            assert_eq!(c.unreadable().next(), Some(0));
            let facts = Facts {
                chain: &w.chain,
                health: w.health,
                heartbeat: heartbeat(),
                remote_order: false,
                now: w.now,
            };
            let screen = c.judge(0, &facts, &mut w.m).unwrap();
            assert_eq!(screen.state, State::NeedsAPerson);
            assert_eq!(screen.why, Why::StoredFormUnreadable);
            assert!(screen.reports.is_empty());
            assert_eq!(screen.state.color(), crate::Color::Red);
            assert!(screen.why_text().contains("cannot be read"));
        }
    }
}

#[test]
fn cr_42_a_sale_stored_under_another_index_does_not_read_back() {
    let mut w = World::new();
    let o = w.open_five();
    let bytes = w.m.slots[&Slot::Sale(o.index)].clone();
    w.m.slots.insert(Slot::Sale(5), bytes);
    let mut c = w.m.reload();
    assert_eq!(c.unreadable().collect::<Vec<_>>(), vec![5]);
    assert!(c.sale(5).is_none());
    let facts = Facts {
        chain: &w.chain,
        health: w.health,
        heartbeat: heartbeat(),
        remote_order: false,
        now: w.now,
    };
    assert_eq!(
        c.judge(5, &facts, &mut w.m).unwrap().state,
        State::NeedsAPerson
    );
    assert_eq!(
        c.judge(0, &facts, &mut w.m).unwrap().state,
        State::NotPaidYet
    );
}

#[test]
fn cr_42_every_sale_with_a_damaged_byte_is_either_refused_or_reads_back_the_same() {
    // Flip each byte of a stored sale: the sale never reads back as
    // something else.
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 2);
    w.judge(o.index);
    let good = w.m.slots[&Slot::Sale(0)].clone();
    let want = crate::Sale::from_bytes(&good).unwrap();
    for i in 0..good.len() {
        let mut m = good.clone();
        m[i] ^= 0x01;
        if let Ok(sale) = crate::Sale::from_bytes(&m) {
            assert_eq!(sale.to_bytes(), m, "byte {i} decoded non-canonically");
            assert_ne!(sale.to_bytes(), want.to_bytes());
        }
    }
}

#[test]
fn cr_42_batches_ledger_pause_and_desk_that_do_not_read_back_refuse_the_start() {
    let mut w = ready_to_cancel();
    w.judge(0);
    for (slot, want) in [
        (Slot::Batches, "batches"),
        (Slot::Exposure, "exposure"),
        (Slot::Pause, "pause"),
        (Slot::Desk, "desk"),
    ] {
        let mut m = Mem {
            slots: w.m.slots.clone(),
            records: w.m.records.clone(),
            ..Mem::default()
        };
        let cut = m.slots[&slot].len() - 1;
        m.slots.get_mut(&slot).unwrap().truncate(cut);
        let err = Counter::load(id(3), pairing(), settings(), m.stored()).unwrap_err();
        assert_eq!(err.rule(), "CR-42");
        let matches = matches!(
            (&err, want),
            (LoadError::Batches(_), "batches")
                | (LoadError::Exposure(_), "exposure")
                | (LoadError::Pause(_), "pause")
                | (LoadError::Desk(_), "desk")
        );
        assert!(matches, "{err:?} for {want}");
        assert!(err.to_string().starts_with("CR-42: "));
    }
}

#[test]
fn cr_42_a_record_chain_that_does_not_verify_refuses_the_start() {
    let mut w = World::new();
    w.open_five();
    w.open_five();
    // Change a byte inside the first record's body.
    let mut m = Mem {
        slots: w.m.slots.clone(),
        records: w.m.records.clone(),
        ..Mem::default()
    };
    let n = m.records[0].len();
    m.records[0][n - 70] ^= 1;
    let stored = m.stored();
    let err = Counter::load(id(3), pairing(), settings(), stored).unwrap_err();
    assert!(matches!(err, LoadError::Records(_)), "{err:?}");
    // A chain signed by another device is refused too.
    let mut other = Chain::new();
    other
        .append(
            &id(9),
            openbmt_core::records::RecordKind::Sale,
            b"x".to_vec(),
        )
        .unwrap();
    let m = Mem {
        slots: w.m.slots.clone(),
        records: other.records().iter().map(Record::to_bytes).collect(),
        ..Mem::default()
    };
    assert!(Counter::load(id(3), pairing(), settings(), m.stored()).is_err());
}

#[test]
fn cr_42_open_refuses_an_address_a_sale_already_holds() {
    // The batches say index 0 is unused (an older copy), but a sale for it
    // is stored: the address is not handed out again.
    let mut w = World::new();
    w.open_five();
    let old_batches = {
        let fresh = World::new();
        fresh.m.slots[&Slot::Batches].clone()
    };
    w.m.slots.insert(Slot::Batches, old_batches);
    w.restart();
    let facts = w.open_facts();
    let err =
        w.c.open(five_dollars(), usd(), rate(), facts, &mut w.m)
            .unwrap_err();
    assert_eq!(err, OpenRefused::AddressInUse);
    assert_eq!(err.rule(), "CR-02");
}

#[test]
fn a_check_in_goes_through_the_stored_path_and_reads_back() {
    let mut w = ready_to_accept();
    let before = w.c.records().len();
    w.m.reset();
    w.m.fail_all = true;
    assert!(w.c.check_in(&mut w.m).is_err());
    assert_eq!(w.c.records().len(), before, "a failed write adds nothing");
    w.m.reset();
    let rec = w.c.check_in(&mut w.m).unwrap();
    assert_eq!(rec.seq, before);
    assert_eq!(rec.check_in_claim().unwrap(), (before, rec.prev));
    assert_eq!(w.c.records().len(), before + 1);
    // The record was stored before it counted, and the chain reads back.
    assert_eq!(w.m.log, vec!["record".to_string()]);
    let again = Counter::load(id(3), pairing(), settings(), w.m.stored()).unwrap();
    assert_eq!(again.records().head(), w.c.records().head());
    assert!(again
        .records()
        .check_against_check_in(&rec, &DeviceList::new([id(3).device_id()]))
        .is_ok());
}
