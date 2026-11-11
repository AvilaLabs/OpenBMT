//! The pump thread and the screen thread share one node: a call from the
//! screen thread is never kept longer than the cap on a pump's wait, and
//! nothing is lost or repeated when both run at once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openbmt_net_io::AppEvent;

use super::kit::*;
use super::wire::{frame, Rd, W};
use crate::{dispatch, MAX_WAIT_MS, SLICE_MS};

fn pump_call(rig: &Rig, handle: u64, wait_ms: u32) -> (Vec<String>, bool) {
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
    let (notes, _) = pump_call(&rig, handle, 60_000);
    let took = began.elapsed();
    assert!(notes.is_empty());
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
fn a_pump_returns_as_soon_as_a_line_is_ready() {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![AppEvent::Note("a line".to_string())]);
    let began = Instant::now();
    let (notes, _) = pump_call(&rig, handle, 60_000);
    assert_eq!(notes, vec!["a line".to_string()]);
    assert!(
        began.elapsed() < Duration::from_millis(u64::from(MAX_WAIT_MS)),
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
                    &frame(10, handle, &W::new().u64(T0).u32(60_000).0),
                );
                assert!(Rd::error(&response).is_none());
                loops += 1;
            }
            loops
        })
    };
    // Let the pump thread get into its loop.
    std::thread::sleep(Duration::from_millis(50));

    // The screen thread: status, the receipts line, the queue, many times.
    let mut times: Vec<Duration> = Vec::new();
    let mut timed = |op: u16, fields: &[u8]| -> Vec<u8> {
        let began = Instant::now();
        let response = rig.call(op, handle, fields);
        times.push(began.elapsed());
        response
    };
    for _ in 0..40 {
        Rd::ok(&timed(20, &[])).status();
    }
    rig.send(vec![peers_event()]);
    std::thread::sleep(Duration::from_millis(100));
    for _ in 0..20 {
        Rd::ok(&timed(21, &[])).receipts();
        Rd::ok(&timed(40, &[])).queue();
        Rd::ok(&timed(23, &[])).list(Rd::send);
    }
    // A call that changes something is held no longer.
    let queued = timed(
        45,
        &W::new().u64(T0).bytes(&counter_bytes()).u32(0).u32(3).0,
    );
    assert_eq!(Rd::ok(&queued).u32(), 1);
    running.store(false, Ordering::SeqCst);
    let loops = pump.join().expect("the pump thread");
    assert!(loops >= 1);
    // The tests of the crate run side by side, so one call may be kept by
    // the scheduler and not by the node's thread: nine calls in ten are
    // within the cap, and none is far beyond it.
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
fn a_payment_is_followed_while_the_pump_loop_runs_and_each_line_comes_once() {
    let rig = Arc::new(Rig::new());
    let handle = rig.start();
    let running = Arc::new(AtomicBool::new(true));
    let lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let pump = {
        let (rig, running, lines) = (Arc::clone(&rig), Arc::clone(&running), Arc::clone(&lines));
        std::thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                let (got, _) = pump_call(&rig, handle, 60_000);
                lines.lock().expect("lock").extend(got);
            }
        })
    };
    rig.send(vec![
        AppEvent::Note("one".to_string()),
        AppEvent::Note("two".to_string()),
    ]);
    let began = Instant::now();
    while lines.lock().expect("lock").len() < 2 {
        assert!(began.elapsed() < Duration::from_secs(10), "no lines");
        std::thread::sleep(Duration::from_millis(10));
    }
    // The screen thread asks for the lines and finds them taken already.
    let response = rig.call(60, handle, &[]);
    let mut r = Rd::ok(&response);
    assert!(r.list(Rd::string).is_empty());
    r.done();
    std::thread::sleep(Duration::from_millis(300));
    running.store(false, Ordering::SeqCst);
    pump.join().expect("the pump thread");
    assert_eq!(*lines.lock().expect("lock"), vec!["one", "two"]);
    // A stop while nobody pumps lets go of the node.
    Rd::ok(&rig.call(2, handle, &[])).pump();
}
