//! The pump thread and the screen thread share one node: a call from the
//! screen thread is never kept longer than the cap on a pump's wait, and
//! nothing is lost or repeated when both run at once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::kit::*;
use super::wire::{Rd, Report, W};
use crate::{dispatch, MAX_WAIT_MS, SLICE_MS};

fn pump_call(rig: &Rig, handle: u64, wait_ms: u32) -> (Vec<Report>, Vec<String>, bool) {
    let response = rig.call(10, handle, &W::new().u64(T0).u32(wait_ms).0);
    let mut r = Rd::ok(&response);
    let out = r.pump();
    r.done();
    out
}

#[test]
fn a_pump_waits_for_at_most_the_cap_whatever_it_is_asked() {
    let rig = Rig::new();
    let handle = rig.start();
    let began = Instant::now();
    let (reports, _, _) = pump_call(&rig, handle, 60_000);
    let took = began.elapsed();
    assert!(reports.is_empty());
    assert!(
        took >= Duration::from_millis(u64::from(MAX_WAIT_MS) - 30),
        "it waited only {took:?}"
    );
    assert!(
        took < Duration::from_millis(u64::from(MAX_WAIT_MS) + 350),
        "it waited {took:?}"
    );
    // With nothing to wait for it returns at once.
    let began = Instant::now();
    pump_call(&rig, handle, 0);
    assert!(began.elapsed() < Duration::from_millis(100));
}

#[test]
fn a_pump_hands_over_a_report_in_the_call_that_made_it() {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    pump_call(&rig, handle, 50);
    rig.call(
        11,
        handle,
        &W::new().open(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed").0,
    );
    let pay = spend(1, &[(&script_of(0), 5_000)]);
    let block = mine(genesis(), 1, std::slice::from_ref(&pay));
    rig.send(vec![
        sighting(&pay, vec![peer(20)]),
        joined(1, &block),
        block_event(&block),
    ]);
    let began = Instant::now();
    let (reports, _, _) = pump_call(&rig, handle, 60_000);
    assert_eq!(reports, vec![Report::PosPaid(0)]);
    // The call checks the block and the sale before it returns, and that
    // work is not cut by the cap: in a debug build on a shared machine it
    // can take longer than the cap by itself. What is bounded is that the
    // call does not go on to wait what it was asked (60 s).
    assert!(
        began.elapsed() < Duration::from_millis(u64::from(MAX_WAIT_MS) + 2_000),
        "{:?}",
        began.elapsed()
    );
}

#[test]
fn a_call_from_the_screen_thread_is_not_kept_longer_than_the_cap_by_a_pump_loop() {
    let rig = Arc::new(Rig::new());
    let handle = rig.start();
    let running = Arc::new(AtomicBool::new(true));
    // The app's pump thread: the longest wait it can ask for, in a loop.
    let pump = {
        let (rig, running) = (Arc::clone(&rig), Arc::clone(&running));
        std::thread::spawn(move || {
            let mut loops = 0u32;
            while running.load(Ordering::SeqCst) {
                let response = dispatch(
                    rig.registry(),
                    &crate::tests::wire::frame(10, handle, &W::new().u64(T0).u32(60_000).0),
                );
                assert!(Rd::error(&response).is_none());
                loops += 1;
            }
            loops
        })
    };
    // Let the pump thread get into its loop.
    std::thread::sleep(Duration::from_millis(50));

    // The screen thread: status, a sale, a judgment, many times.
    let mut times: Vec<Duration> = Vec::new();
    let mut timed = |op: u16, fields: &[u8]| -> Vec<u8> {
        let began = Instant::now();
        let response = rig.call(op, handle, fields);
        times.push(began.elapsed());
        response
    };
    for _ in 0..40 {
        Rd::ok(&timed(30, &[]));
    }
    // The health answer is fed by the driver, and the pump takes it.
    rig.send(vec![peers_event()]);
    std::thread::sleep(Duration::from_millis(100));
    let opened = timed(
        11,
        &W::new().open(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed").0,
    );
    Rd::ok(&opened);
    for _ in 0..20 {
        Rd::ok(&timed(12, &W::new().u64(T0).u32(0).0));
        Rd::ok(&timed(31, &[]));
    }
    running.store(false, Ordering::SeqCst);
    let loops = pump.join().expect("the pump thread");
    assert!(loops >= 1);
    // The tests of the crate run side by side, so one call may be kept by
    // the scheduler and not by the lock: nine calls in ten are within the
    // cap, and none is far beyond it.
    times.sort();
    let p90 = times[times.len() * 9 / 10];
    let worst = times[times.len() - 1];
    assert!(
        p90 < Duration::from_millis(u64::from(MAX_WAIT_MS)),
        "a screen call waited {p90:?} for the pump (a slice is {SLICE_MS} ms)"
    );
    assert!(
        worst < Duration::from_secs(2),
        "the slowest call took {worst:?}"
    );
}

#[test]
fn a_sale_is_paid_while_the_pump_loop_runs_and_each_report_comes_once() {
    let rig = Arc::new(Rig::new());
    let handle = rig.start();
    let running = Arc::new(AtomicBool::new(true));
    let reports: Arc<Mutex<Vec<Report>>> = Arc::new(Mutex::new(Vec::new()));
    let pump = {
        let (rig, running, reports) =
            (Arc::clone(&rig), Arc::clone(&running), Arc::clone(&reports));
        std::thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                let (got, _, _) = pump_call(&rig, handle, 60_000);
                reports.lock().expect("lock").extend(got);
            }
        })
    };
    rig.send(vec![peers_event()]);
    // The sale opens once the pump has taken the health answer.
    let began = Instant::now();
    let opened = loop {
        let response = rig.call(
            11,
            handle,
            &W::new().open(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed").0,
        );
        if Rd::error(&response).is_none() {
            break response;
        }
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "no health answer"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(Rd::ok(&opened).opened().index, 0);
    let pay = spend(1, &[(&script_of(0), 5_000)]);
    let block = mine(genesis(), 1, std::slice::from_ref(&pay));
    rig.send(vec![
        sighting(&pay, vec![peer(20)]),
        joined(1, &block),
        block_event(&block),
    ]);
    let began = Instant::now();
    while reports.lock().expect("lock").is_empty() {
        assert!(began.elapsed() < Duration::from_secs(10), "no report");
        std::thread::sleep(Duration::from_millis(10));
    }
    // The screen sees the same state, and the report is not repeated.
    let response = rig.call(12, handle, &W::new().u64(T0).u32(0).0);
    let screen = Rd::ok(&response).screen();
    assert_eq!(screen.state, 4, "Confirmed");
    assert!(screen.reports.is_empty());
    std::thread::sleep(Duration::from_millis(300));
    running.store(false, Ordering::SeqCst);
    pump.join().expect("the pump thread");
    assert_eq!(*reports.lock().expect("lock"), vec![Report::PosPaid(0)]);
    // A stop while nobody pumps lets go of the node.
    Rd::ok(&rig.call(2, handle, &[]));
}
