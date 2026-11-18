//! The Box running on real sockets on the loopback: BX-01 to BX-03 and the
//! parts of BX-12 to BX-24 that are about what a device on the network sees.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use openbmt_core::boxpair::{Claim, Invitation, Welcome};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_link::proto::Refusal;
use openbmt_link::{
    open, os_random, seal, Client, ClientConfig, ClientError, Kind, Role, Route, ServerConfig,
};
use openbmt_net_io::Clock;

use super::kit::*;
use crate::runner::{invitation_text, BoxRunner, StartFailure};
use crate::state::{BoxError, BoxStatus};

pub struct TestClock(AtomicU64);

impl TestClock {
    pub fn new(at: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(at)))
    }

    pub fn advance(&self, secs: u64) {
        self.0.fetch_add(secs, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

pub(super) fn start_in(dir: &TempDir, clock: &Arc<TestClock>) -> Result<BoxRunner, StartFailure> {
    BoxRunner::start(
        dir.path(),
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        Arc::clone(clock) as Arc<dyn Clock + Send + Sync>,
        ServerConfig::default(),
    )
}

pub(super) fn start(dir: &TempDir) -> (BoxRunner, Arc<TestClock>) {
    let clock = TestClock::new(NOW);
    let runner = start_in(dir, &clock).unwrap();
    (runner, clock)
}

pub(super) fn connect(r: &BoxRunner) -> Client {
    Client::connect(
        &Route::Direct(r.local_addr()),
        Some(&r.device_id()),
        &ClientConfig::default(),
    )
    .unwrap()
}

/// Wait up to ten seconds for `ok` to hold.
pub(super) fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("never happened: {what}");
}

/// The Key app and the phone: enter the invitation at the Box, fetch the
/// claim, check it, make the welcome and send it with the settings.
fn pair(r: &BoxRunner, inv: &Invitation, settings: &SignedSettings) -> Result<(), ClientError> {
    r.enter_invitation(&inv.to_text()).unwrap();
    let mut c = connect(r);
    let claim = Claim::from_bytes(&c.claim().unwrap()).unwrap();
    assert!(claim.verify(inv));
    assert_eq!(claim.box_key, r.device_id());
    let welcome = Welcome::make(inv, &claim, roots());
    c.welcome(&welcome.to_bytes(), &settings.to_bytes())
}

pub(super) fn pair_ok(r: &BoxRunner) {
    let inv = invitation(5);
    let s = sign_both(settings_with(1, &r.device_id()));
    pair(r, &inv, &s).unwrap();
    assert_eq!(r.status(), BoxStatus::Paired);
}

fn rng(buf: &mut [u8; 32]) -> std::io::Result<()> {
    os_random(buf)
}

/// A sealed message with an explicit number.
fn sealed(
    from: &Identity,
    from_role: Role,
    to: &Identity,
    to_role: Role,
    n: u64,
    kind: Kind,
    content: &[u8],
) -> Vec<u8> {
    sealed_to(from, from_role, &to.device_id(), to_role, n, kind, content)
}

pub(super) fn sealed_to(
    from: &Identity,
    from_role: Role,
    to: &DeviceId,
    to_role: Role,
    n: u64,
    kind: Kind,
    content: &[u8],
) -> Vec<u8> {
    seal(from, from_role, to, to_role, n, kind, content, &mut rng).unwrap()
}

pub(super) fn mailbox(r: &BoxRunner, who: &Identity) -> Result<(), ClientError> {
    connect(r).make_mailbox(who)
}

fn refused(e: ClientError) -> Refusal {
    match e {
        ClientError::Refused(r) => r,
        other => panic!("not a refusal: {other}"),
    }
}

// ---- section 1 ----

#[test]
fn bx_01_the_relays_key_is_the_boxs_key_and_it_survives_a_restart() {
    let dir = TempDir::new("key");
    let (r, clock) = start(&dir);
    let key = r.device_id();
    // The hello is signed by it; connect checks the signature and the key.
    assert_eq!(*connect(&r).relay(), key);
    r.stop();
    let r = start_in(&dir, &clock).unwrap();
    assert_eq!(r.device_id(), key);
    assert_eq!(*connect(&r).relay(), key);
    assert_eq!(r.fingerprint_words(), key.fingerprint_words());
}

#[test]
fn bx_02_one_process_uses_a_data_directory_at_a_time() {
    let dir = TempDir::new("lock");
    let (r, clock) = start(&dir);
    let listing = || {
        let mut all = Vec::new();
        let mut todo = vec![dir.path().to_path_buf()];
        while let Some(d) = todo.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    todo.push(p);
                } else {
                    all.push((p.clone(), std::fs::read(&p).unwrap()));
                }
            }
        }
        all.sort();
        all
    };
    let before = listing();
    let second = start_in(&dir, &clock);
    assert!(
        matches!(second, Err(StartFailure::Lock(_))),
        "a second Box on the same directory started"
    );
    // The refused start read and wrote nothing in the directory.
    assert_eq!(listing(), before);
    r.stop();
    // Once the first is gone, the directory can be used again.
    assert!(start_in(&dir, &clock).is_ok());
}

#[test]
fn bx_03_a_stranger_can_do_nothing_to_an_unpaired_box_but_hello_and_claim() {
    let dir = TempDir::new("stranger");
    let (r, _) = start(&dir);
    let mut c = connect(&r); // hello works
                             // With no invitation open there is no claim.
    assert_eq!(refused(c.claim().unwrap_err()), Refusal::NoClaim);
    // No challenge, so no mailbox, no fetch, no ack.
    assert_eq!(
        refused(c.make_mailbox(&stranger()).unwrap_err()),
        Refusal::NotMember
    );
    assert_eq!(
        refused(c.fetch(&stranger(), 0).unwrap_err()),
        Refusal::NotMember
    );
    assert_eq!(
        refused(c.ack(&stranger(), 0).unwrap_err()),
        Refusal::NotMember
    );
    // No sealed message is taken, from anyone.
    let m = sealed(
        &owner(),
        Role::Owner,
        &counter1(),
        Role::Counter,
        1,
        Kind::Heartbeat,
        b"x",
    );
    assert_eq!(refused(c.put(&m).unwrap_err()), Refusal::SenderNotMember);
    // A welcome with no invitation open is refused.
    assert_eq!(
        refused(c.welcome(&[1, 2, 3], &[4]).unwrap_err()),
        Refusal::WelcomeRefused
    );
    // With an invitation open the claim is offered and nothing else is.
    r.enter_invitation(&invitation(5).to_text()).unwrap();
    assert!(c.claim().is_ok());
    assert_eq!(
        refused(c.make_mailbox(&stranger()).unwrap_err()),
        Refusal::NotMember
    );
    assert_eq!(refused(c.put(&m).unwrap_err()), Refusal::SenderNotMember);
    assert!(r.members().is_empty());
}

// ---- section 2 ----

#[test]
fn bx_12_a_claim_request_with_no_invitation_open_is_refused() {
    let dir = TempDir::new("noclaim");
    let (r, _) = start(&dir);
    assert_eq!(refused(connect(&r).claim().unwrap_err()), Refusal::NoClaim);
}

#[test]
fn bx_12_the_claim_goes_out_on_the_wire_and_runs_out_after_thirty_minutes() {
    let dir = TempDir::new("claimwire");
    let (r, clock) = start(&dir);
    let inv = invitation(5);
    r.enter_invitation(&inv.to_text()).unwrap();
    let mut c = connect(&r);
    let bytes = c.claim().unwrap();
    let claim = Claim::from_bytes(&bytes).unwrap();
    assert!(claim.verify(&inv));
    // Asking again gives the same claim: the nonce stays while it is open.
    assert_eq!(c.claim().unwrap(), bytes);
    clock.advance(30 * 60);
    eventually("the claim stops being offered", || {
        connect(&r).claim() == Err(ClientError::Refused(Refusal::NoClaim))
    });
    assert_eq!(r.status(), BoxStatus::Unpaired);
}

#[test]
fn bx_12_the_invitation_file_gives_its_first_line_trimmed() {
    assert_eq!(invitation_text("  ABC DEF \nsecond line\n"), "ABC DEF");
    assert_eq!(invitation_text("\r\nABC"), "");
    assert_eq!(invitation_text(""), "");
}

#[test]
fn bx_12_a_typo_in_the_invitation_is_refused_at_the_box() {
    let dir = TempDir::new("typo");
    let (r, _) = start(&dir);
    let text = invitation(5).to_text();
    let mut typo = text.clone();
    typo.replace_range(0..1, if &text[..1] == "0" { "1" } else { "0" });
    assert!(matches!(
        r.enter_invitation(&typo),
        Err(BoxError::Invitation(_))
    ));
    assert_eq!(r.status(), BoxStatus::Unpaired);
    assert_eq!(refused(connect(&r).claim().unwrap_err()), Refusal::NoClaim);
}

#[test]
fn bx_15_a_welcome_over_the_wire_pairs_the_box_and_opens_the_relay_to_its_members() {
    let dir = TempDir::new("pairwire");
    let (r, _) = start(&dir);
    pair_ok(&r);
    let mut want = vec![
        owner().device_id(),
        counter1().device_id(),
        counter2().device_id(),
        r.device_id(),
    ];
    want.sort();
    let mut got = r.members();
    got.sort();
    assert_eq!(got, want);
    // The invitation is spent: no claim, and a paired Box takes no welcome.
    assert_eq!(refused(connect(&r).claim().unwrap_err()), Refusal::NoClaim);
    assert_eq!(
        refused(
            connect(&r)
                .welcome(
                    &[1; 101],
                    &sign_both(settings_with(2, &r.device_id())).to_bytes()
                )
                .unwrap_err()
        ),
        Refusal::WelcomeRefused
    );
    assert!(matches!(
        r.enter_invitation(&invitation(6).to_text()),
        Err(BoxError::AlreadyPaired)
    ));
}

#[test]
fn bx_16_o13_01_a_lost_first_answer_is_answered_again_over_the_wire_and_a_different_welcome_is_not()
{
    let dir = TempDir::new("welcomeretry");
    let (r, _) = start(&dir);
    let inv = invitation(5);
    let settings = sign_both(settings_with(1, &r.device_id())).to_bytes();
    r.enter_invitation(&inv.to_text()).unwrap();
    let claim = Claim::from_bytes(&connect(&r).claim().unwrap()).unwrap();
    let welcome = Welcome::make(&inv, &claim, roots()).to_bytes();
    // The first answer is lost on its way back: the connection is dropped.
    {
        let mut c = connect(&r);
        c.welcome(&welcome, &settings).unwrap();
    }
    assert_eq!(r.status(), BoxStatus::Paired);
    let members = r.members();
    // The phone, not knowing it was taken, sends the same bytes again.
    connect(&r).welcome(&welcome, &settings).unwrap();
    assert_eq!(r.status(), BoxStatus::Paired);
    assert_eq!(r.members(), members);
    // Another welcome, or other settings, are refused as before.
    let other = sign_both(settings_with(2, &r.device_id())).to_bytes();
    assert_eq!(
        refused(connect(&r).welcome(&welcome, &other).unwrap_err()),
        Refusal::WelcomeRefused
    );
    assert_eq!(
        refused(connect(&r).welcome(&[1; 101], &settings).unwrap_err()),
        Refusal::WelcomeRefused
    );
    assert_eq!(r.status(), BoxStatus::Paired);
}

#[test]
fn bx_15_every_failed_welcome_has_the_one_refusal_and_only_a_checked_mac_spends() {
    let inv = invitation(5);
    let dir = TempDir::new("welcomefail");
    let (r, clock) = start(&dir);
    let me = r.device_id();
    let good_settings = sign_both(settings_with(1, &me));
    let other_inv = invitation(6);
    let unlisted = sign_both(settings_with(1, &stranger().device_id()));
    let other_roots =
        openbmt_core::batch::PinnedRoots::new(stranger().device_id(), approver().device_id())
            .unwrap();
    // (welcome, settings, spends the invitation)
    for n in 0..5 {
        // Each case starts from a fresh invitation and its claim.
        r.enter_invitation(&inv.to_text()).unwrap();
        let mut c = connect(&r);
        let claim_bytes = c.claim().unwrap_or_else(|e| panic!("case {n}: {e:?}"));
        let claim = Claim::from_bytes(&claim_bytes).unwrap();
        let (w, s, spends) = match n {
            // Made for another invitation.
            0 => (
                Welcome::make(&other_inv, &claim, roots()).to_bytes(),
                good_settings.to_bytes(),
                false,
            ),
            // Right MAC, for roots the settings were not signed by.
            1 => (
                Welcome::make(&inv, &claim, other_roots).to_bytes(),
                good_settings.to_bytes(),
                true,
            ),
            // Right MAC, settings that do not list the Box.
            2 => (
                Welcome::make(&inv, &claim, roots()).to_bytes(),
                unlisted.to_bytes(),
                true,
            ),
            // Garbage.
            3 => (vec![0; 101], vec![0; 10], false),
            _ => (vec![], vec![], false),
        };
        clock.advance(2);
        assert_eq!(
            refused(c.welcome(&w, &s).unwrap_err()),
            Refusal::WelcomeRefused,
            "case {n}"
        );
        if spends {
            assert_eq!(
                refused(c.claim().unwrap_err()),
                Refusal::NoClaim,
                "case {n}"
            );
            assert_eq!(r.status(), BoxStatus::Unpaired);
        } else {
            // Nothing changed: the same claim is still on offer.
            assert_eq!(c.claim().unwrap(), claim_bytes, "case {n}");
            assert_eq!(r.status(), BoxStatus::InvitationOpen);
        }
        assert!(r.members().is_empty());
        clock.advance(2);
    }
}

#[test]
fn bx_15_a_sync_racing_a_new_invitation_never_hides_its_claim() {
    // The worker syncs the relay with the core every step. A sync that read
    // the clock just before the time moved on and a new invitation was
    // entered would find the invitation entered in its future, which counts
    // as expired, and forget it.
    let inv = invitation(5);
    let dir = TempDir::new("syncrace");
    let (r, clock) = start(&dir);
    let other_roots =
        openbmt_core::batch::PinnedRoots::new(stranger().device_id(), approver().device_id())
            .unwrap();
    let settings = sign_both(settings_with(1, &r.device_id())).to_bytes();
    // Stops the spinning threads however the loop ends, a panic included.
    struct Stop<'a>(&'a std::sync::atomic::AtomicBool);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let done = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        let _stop = Stop(&done);
        for _ in 0..4 {
            scope.spawn(|| {
                while !done.load(Ordering::SeqCst) {
                    r.sync_now();
                }
            });
        }
        for n in 0..300 {
            clock.advance(1);
            r.enter_invitation(&inv.to_text()).unwrap();
            let mut c = connect(&r);
            let claim_bytes = c.claim().unwrap_or_else(|e| panic!("round {n}: {e:?}"));
            let claim = Claim::from_bytes(&claim_bytes).unwrap();
            // Right MAC for roots the settings were not signed by: spends.
            let w = Welcome::make(&inv, &claim, other_roots).to_bytes();
            assert_eq!(
                refused(c.welcome(&w, &settings).unwrap_err()),
                Refusal::WelcomeRefused
            );
        }
    });
}

#[test]
fn bx_15_a_welcome_waiting_for_the_core_never_withdraws_an_invitation_entered_meanwhile() {
    // The relay reads the time before the hook waits for the core's lock. A
    // hook that used that time would find an invitation entered during the
    // wait in its future, count it as expired and withdraw its claim.
    let inv = invitation(6);
    let dir = TempDir::new("welcomewait");
    let (r, clock) = start(&dir);
    r.enter_invitation(&inv.to_text()).unwrap();
    std::thread::scope(|scope| {
        r.with_core(|core| {
            // A welcome that is not for any invitation, now waiting in the
            // hook for this lock with the time it was sent at.
            scope.spawn(|| {
                let mut c = connect(&r);
                c.welcome(&[1, 2, 3], &[4])
            });
            std::thread::sleep(Duration::from_millis(400));
            clock.advance(1);
            core.enter_invitation(&inv.to_text(), clock.now(), [9u8; 32])
                .unwrap();
        });
    });
    eventually("the new claim is still on offer", || {
        connect(&r).claim().is_ok()
    });
}

#[test]
fn bx_15_a_stranger_welcoming_over_the_wire_cannot_stop_the_real_pairing() {
    let dir = TempDir::new("strangerwelcome");
    let (r, clock) = start(&dir);
    let inv = invitation(5);
    r.enter_invitation(&inv.to_text()).unwrap();
    let mut stranger_conn = connect(&r);
    let claim_bytes = stranger_conn.claim().unwrap();
    // Within one second only the first is looked at; all are refused alike.
    for _ in 0..50 {
        assert_eq!(
            refused(stranger_conn.welcome(&[7; 101], &[8; 50]).unwrap_err()),
            Refusal::WelcomeRefused
        );
    }
    clock.advance(2);
    assert_eq!(stranger_conn.claim().unwrap(), claim_bytes);
    // The real welcome pairs with the claim already held.
    let claim = Claim::from_bytes(&claim_bytes).unwrap();
    let welcome = Welcome::make(&inv, &claim, roots());
    let settings = sign_both(settings_with(1, &r.device_id()));
    connect(&r)
        .welcome(&welcome.to_bytes(), &settings.to_bytes())
        .unwrap();
    assert_eq!(r.status(), BoxStatus::Paired);
}

// ---- sections 3 and 5 ----

fn all_but_c2(me: &openbmt_core::identity::DeviceId, version: u32) -> Settings {
    let mut s = settings_with(version, me);
    s.devices.remove(&counter2().device_id());
    s.counters.remove(&counter2().device_id());
    s
}

#[test]
fn bx_22_devices_on_the_list_pass_sealed_messages_through_the_box_and_others_cannot() {
    let dir = TempDir::new("pass");
    let (r, _) = start(&dir);
    pair_ok(&r);
    mailbox(&r, &counter1()).unwrap();
    mailbox(&r, &counter2()).unwrap();
    assert_eq!(
        refused(mailbox(&r, &stranger()).unwrap_err()),
        Refusal::NotMember
    );
    // counter 1 to the owner? Not a kind counters send to the owner except
    // check-in and records: use a check-in.
    mailbox(&r, &owner()).unwrap();
    let m = sealed(
        &counter1(),
        Role::Counter,
        &owner(),
        Role::Owner,
        1,
        Kind::CheckIn,
        b"hello",
    );
    connect(&r).put(&m).unwrap();
    let mut c = connect(&r);
    let got = c.fetch(&owner(), 0).unwrap();
    assert_eq!(got.items.len(), 1);
    let opened = open(
        &owner(),
        Role::Owner,
        &got.items[0].1,
        |d| (*d == counter1().device_id()).then_some(Role::Counter),
        |_| 0,
    )
    .unwrap();
    assert_eq!(opened.content, b"hello");
    // A stranger's sealed message is refused.
    let bad = sealed(
        &stranger(),
        Role::Counter,
        &owner(),
        Role::Owner,
        1,
        Kind::CheckIn,
        b"x",
    );
    assert_eq!(
        refused(connect(&r).put(&bad).unwrap_err()),
        Refusal::SenderNotMember
    );
}

/// The owner sends settings to the Box as a sealed message; the relay needs
/// the Box's mailbox first, which the Box makes on its own.
pub(super) fn send_settings(r: &BoxRunner, number: u64, settings: &SignedSettings) {
    let m = sealed_to(
        &owner(),
        Role::Owner,
        &r.device_id(),
        Role::Box,
        number,
        Kind::Settings,
        &settings.to_bytes(),
    );
    eventually("the Box has a mailbox", || connect(r).put(&m).is_ok());
}

#[test]
fn bx_21_new_settings_arrive_as_a_sealed_message_and_take_a_device_off_the_list() {
    let dir = TempDir::new("settingsmsg");
    let (r, _) = start(&dir);
    pair_ok(&r);
    mailbox(&r, &counter2()).unwrap();
    assert!(connect(&r).fetch(&counter2(), 0).is_ok());
    let me = r.device_id();
    send_settings(&r, 1, &sign_owner(all_but_c2(&me, 2)));
    eventually("the device list changes", || {
        !r.members().contains(&counter2().device_id())
    });
    // Its mailbox is leaving (LK-28): one fetch reads it to its end (LK-29),
    // and then it is gone and the device is refused.
    assert!(connect(&r)
        .fetch(&counter2(), 0)
        .is_ok_and(|got| got.items.is_empty()));
    assert_eq!(
        refused(connect(&r).fetch(&counter2(), 0).unwrap_err()),
        Refusal::NoMailbox
    );
    assert_eq!(
        refused(mailbox(&r, &counter2()).unwrap_err()),
        Refusal::NotMember
    );
    assert_eq!(r.core().lock().unwrap().settings().unwrap().version, 2);
    // Counter 1 still works.
    assert!(mailbox(&r, &counter1()).is_ok());
}

#[test]
fn bx_21_replayed_and_forged_settings_messages_change_nothing() {
    let dir = TempDir::new("replay");
    let (r, _) = start(&dir);
    pair_ok(&r);
    let me = r.device_id();
    send_settings(&r, 1, &sign_owner(all_but_c2(&me, 2)));
    eventually("version 2", || {
        r.core()
            .lock()
            .unwrap()
            .settings()
            .is_some_and(|s| s.version == 2)
    });
    // Version 1 again, in a message with a new number: stale.
    send_settings(&r, 2, &sign_both(settings_with(1, &me)));
    // Signed by other roots.
    let s = settings_with(3, &me);
    let forged = SignedSettings {
        owner_sig: s.sign(&stranger()),
        approver_sig: Some(s.sign(&approver())),
        settings: s,
        wallet_sig: None,
    };
    send_settings(&r, 3, &forged);
    eventually("both refusals are noted", || {
        r.notes()
            .iter()
            .filter(|n| n.contains("settings from the owner refused"))
            .count()
            >= 2
    });
    assert_eq!(r.core().lock().unwrap().settings().unwrap().version, 2);
    assert!(!r.members().contains(&counter2().device_id()));
}

#[test]
fn bx_24_a_counter_cannot_send_settings_and_other_kinds_are_not_handled_yet() {
    let dir = TempDir::new("roles");
    let (r, _) = start(&dir);
    pair_ok(&r);
    let me = r.device_id();
    let to_box = |from: &Identity, role: Role, n: u64, kind: Kind, content: &[u8]| {
        sealed_to(from, role, &me, Role::Box, n, kind, content)
    };
    // The relay takes a message by signature and membership; the Box's role
    // check (LK-08) is when it opens it.
    let settings = sign_both(settings_with(2, &me)).to_bytes();
    // A counter's key sealing as if it were the owner: the role the Box gives
    // the key is what counts.
    let from_counter = to_box(&counter1(), Role::Owner, 1, Kind::Settings, &settings);
    let list = to_box(&owner(), Role::Owner, 1, Kind::DeviceList, b"x");
    eventually("the Box has a mailbox", || {
        connect(&r).put(&from_counter).is_ok()
    });
    connect(&r).put(&list).unwrap();
    eventually("both are noted", || {
        let notes = r.notes();
        notes
            .iter()
            .any(|n| n.contains("DeviceList") && n.contains("not handled yet"))
            && notes.iter().any(|n| n.contains("refused"))
    });
    assert_eq!(r.core().lock().unwrap().settings().unwrap().version, 1);
}

#[test]
fn bx_23_settings_that_drop_the_box_turn_it_off_over_the_wire() {
    let dir = TempDir::new("off");
    let (r, _) = start(&dir);
    pair_ok(&r);
    mailbox(&r, &counter1()).unwrap();
    let mut s = settings_with(2, &r.device_id());
    s.devices.remove(&r.device_id());
    s.boxes.clear();
    send_settings(&r, 1, &sign_owner(s));
    eventually("the Box is off", || r.status() == BoxStatus::TurnedOff);
    eventually("the member list is empty", || r.members().is_empty());
    // Nothing works any more: no mailbox, no message.
    assert_eq!(
        refused(mailbox(&r, &counter1()).unwrap_err()),
        Refusal::NotMember
    );
    assert_eq!(
        refused(connect(&r).fetch(&counter1(), 0).unwrap_err()),
        Refusal::NotMember
    );
}

#[test]
fn bx_16_a_restart_on_the_same_directory_is_still_paired_with_the_same_key_and_members() {
    let dir = TempDir::new("restart");
    let (r, clock) = start(&dir);
    pair_ok(&r);
    let key = r.device_id();
    let mut members = r.members();
    members.sort();
    r.stop();
    let r = start_in(&dir, &clock).unwrap();
    assert_eq!(r.status(), BoxStatus::Paired);
    assert_eq!(r.device_id(), key);
    let mut again = r.members();
    again.sort();
    assert_eq!(again, members);
    // No claim is offered and a welcome is refused.
    assert_eq!(refused(connect(&r).claim().unwrap_err()), Refusal::NoClaim);
}

#[test]
fn bx_51_a_paired_box_whose_files_are_damaged_does_not_start() {
    let dir = TempDir::new("damage");
    let (r, clock) = start(&dir);
    pair_ok(&r);
    r.stop();
    // The roots file damaged.
    let roots_path = dir.path().join("roots");
    let good = std::fs::read(&roots_path).unwrap();
    std::fs::write(&roots_path, b"junk").unwrap();
    match start_in(&dir, &clock) {
        Err(StartFailure::State(e)) => assert!(e.to_string().contains("roots"), "{e}"),
        _ => panic!("started on damaged roots"),
    }
    std::fs::write(&roots_path, &good).unwrap();
    // The number files gone: the Box needs a new pairing (LK-40).
    std::fs::remove_file(dir.path().join("link-inbox")).unwrap();
    match start_in(&dir, &clock) {
        Err(StartFailure::Courier(e)) => assert!(e.to_string().contains("link-inbox"), "{e}"),
        _ => panic!("started without its number file"),
    }
    // The damaged key.
    std::fs::write(dir.path().join("box.key"), b"junk").unwrap();
    match start_in(&dir, &clock) {
        Err(StartFailure::State(e)) => assert!(e.to_string().contains("box.key"), "{e}"),
        _ => panic!("started on a damaged key"),
    }
}

#[test]
fn bx_52_a_temporary_file_left_by_a_killed_write_is_ignored_and_then_replaced() {
    let dir = TempDir::new("leftover");
    let (r, clock) = start(&dir);
    pair_ok(&r);
    let key = r.device_id();
    r.stop();
    // A write cut short leaves a temporary file next to the whole one.
    for name in ["box.key", "roots", "settings", "link-inbox", "link-outbox"] {
        std::fs::write(dir.path().join(format!("{name}.tmp")), b"half").unwrap();
    }
    let again = start_in(&dir, &clock).unwrap();
    assert_eq!(again.device_id(), key);
    assert_eq!(again.status(), BoxStatus::Paired);
}

#[test]
fn bx_52_the_files_are_whole_after_pairing_when_nothing_more_is_written() {
    let dir = TempDir::new("whole");
    let (r, clock) = start(&dir);
    pair_ok(&r);
    let mut before = r.members();
    // The runner is dropped without stop(): nothing more is written, as
    // after a kill.
    drop(r);
    let again = start_in(&dir, &clock).unwrap();
    assert_eq!(again.status(), BoxStatus::Paired);
    let mut after = again.members();
    before.sort();
    after.sort();
    assert_eq!(after, before);
}

#[test]
fn bx_50_the_files_of_a_running_box_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("private");
    let (r, _) = start(&dir);
    pair_ok(&r);
    for name in ["box.key", "roots", "settings", "link-inbox", "link-outbox"] {
        let mode = std::fs::metadata(dir.path().join(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{name} is readable by others");
    }
    let _ = Settings::defaults(1);
}

fn start_on(dir: &TempDir, bind: &str) -> BoxRunner {
    BoxRunner::start(
        dir.path(),
        bind.parse::<SocketAddr>().unwrap(),
        TestClock::new(NOW) as Arc<dyn Clock + Send + Sync>,
        ServerConfig::default(),
    )
    .unwrap()
}

/// A whole handshake with the Box at `addr`, not only a connect.
fn handshake(r: &BoxRunner, addr: SocketAddr) {
    Client::connect(
        &Route::Direct(addr),
        Some(&r.device_id()),
        &ClientConfig::default(),
    )
    .unwrap();
}

fn bindv6only() -> bool {
    std::fs::read_to_string("/proc/sys/net/ipv6/bindv6only")
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

#[test]
fn bx_53_a_box_bound_to_the_ipv6_loopback_answers_over_ipv6() {
    let dir = TempDir::new("v6lo");
    let r = start_on(&dir, "[::1]:0");
    assert!(r.local_addr().is_ipv6());
    handshake(&r, r.local_addr());
}

#[test]
fn bx_53_a_box_bound_to_the_ipv6_any_address_answers_over_ipv6() {
    let dir = TempDir::new("v6any");
    let r = start_on(&dir, "[::]:0");
    assert!(r.local_addr().is_ipv6());
    let port = r.local_addr().port();
    handshake(&r, SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)));
}

#[test]
fn bx_53_a_box_bound_to_the_ipv6_any_address_answers_over_ipv4_unless_the_host_says_otherwise() {
    // Linux makes `[::]` dual-stack unless net.ipv6.bindv6only is 1; the Box
    // does not change that, so the test follows the host's setting.
    let dir = TempDir::new("v6dual");
    let r = start_on(&dir, "[::]:0");
    let v4 = SocketAddr::from(([127, 0, 0, 1], r.local_addr().port()));
    if bindv6only() {
        assert!(std::net::TcpStream::connect(v4).is_err());
    } else {
        handshake(&r, v4);
    }
}

#[test]
fn bx_53_a_box_bound_to_the_ipv4_any_address_still_answers() {
    let dir = TempDir::new("v4any");
    let r = start_on(&dir, "0.0.0.0:0");
    assert!(r.local_addr().is_ipv4());
    handshake(
        &r,
        SocketAddr::from(([127, 0, 0, 1], r.local_addr().port())),
    );
}

#[test]
fn bx_63_the_settings_in_force_sent_again_to_a_running_box_say_nothing_and_change_nothing() {
    let dir = TempDir::new("settingsagain");
    let (r, _) = start(&dir);
    pair_ok(&r);
    let me = r.device_id();
    let mut v2 = settings_with(2, &me);
    v2.tor_on = true;
    let v2 = sign_both(v2);
    send_settings(&r, 1, &v2);
    eventually("version 2 is taken", || {
        r.with_core(|c| c.settings().map(|s| s.version)) == Some(2)
    });
    let notes = r.notes().len();
    let file = std::fs::read(dir.path().join(crate::state::SETTINGS_FILE)).unwrap();
    // The owner sends them again, twice, and then newer ones: the Box
    // took the newer ones, so the two before them left no note.
    send_settings(&r, 2, &v2);
    send_settings(&r, 3, &v2);
    let mut v3 = settings_with(3, &me);
    v3.tor_on = true;
    send_settings(&r, 4, &sign_both(v3));
    eventually("version 3 is taken", || {
        r.with_core(|c| c.settings().map(|s| s.version)) == Some(3)
    });
    assert_eq!(r.notes().len(), notes, "{:?}", r.notes());
    assert_ne!(
        std::fs::read(dir.path().join(crate::state::SETTINGS_FILE)).unwrap(),
        file
    );
    // And the same again, now of version 3: no note and the same file.
    let file = std::fs::read(dir.path().join(crate::state::SETTINGS_FILE)).unwrap();
    let mut v3 = settings_with(3, &me);
    v3.tor_on = true;
    send_settings(&r, 5, &sign_both(v3));
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(r.notes().len(), notes, "{:?}", r.notes());
    assert_eq!(
        std::fs::read(dir.path().join(crate::state::SETTINGS_FILE)).unwrap(),
        file
    );
    assert_eq!(r.status(), BoxStatus::Paired);
    r.stop();
}
