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
use crate::runner::{BoxRunner, StartFailure};
use crate::state::{BoxError, BoxStatus, POS_TOKEN_FILE};

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

fn start_in(dir: &TempDir, clock: &Arc<TestClock>) -> Result<BoxRunner, StartFailure> {
    BoxRunner::start(
        dir.path(),
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        Arc::clone(clock) as Arc<dyn Clock + Send + Sync>,
        ServerConfig::default(),
    )
}

fn start(dir: &TempDir) -> (BoxRunner, Arc<TestClock>) {
    let clock = TestClock::new(NOW);
    let runner = start_in(dir, &clock).unwrap();
    (runner, clock)
}

fn connect(r: &BoxRunner) -> Client {
    Client::connect(
        &Route::Direct(r.local_addr()),
        Some(&r.device_id()),
        &ClientConfig::default(),
    )
    .unwrap()
}

/// Wait up to ten seconds for `ok` to hold.
fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
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

fn pair_ok(r: &BoxRunner) {
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

fn sealed_to(
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

fn mailbox(r: &BoxRunner, who: &Identity) -> Result<(), ClientError> {
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
    let second = start_in(&dir, &clock);
    assert!(
        matches!(second, Err(StartFailure::Lock(_))),
        "a second Box on the same directory started"
    );
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
        let claim_bytes = c.claim().unwrap();
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
fn send_settings(r: &BoxRunner, number: u64, settings: &SignedSettings) {
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
    // Its mailbox is gone and it is refused.
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
    let paid = to_box(&counter1(), Role::Counter, 2, Kind::PaidNotice, b"x");
    eventually("the Box has a mailbox", || {
        connect(&r).put(&from_counter).is_ok()
    });
    connect(&r).put(&paid).unwrap();
    eventually("both are noted", || {
        let notes = r.notes();
        notes
            .iter()
            .any(|n| n.contains("PaidNotice") && n.contains("not handled yet"))
            && notes.iter().any(|n| n.contains("refused"))
    });
    assert_eq!(r.core().lock().unwrap().settings().unwrap().version, 1);
}

#[test]
fn bx_23_settings_that_drop_the_box_turn_it_off_over_the_wire() {
    let dir = TempDir::new("off");
    let (r, _) = start(&dir);
    pair_ok(&r);
    std::fs::write(dir.path().join(POS_TOKEN_FILE), b"token").unwrap();
    mailbox(&r, &counter1()).unwrap();
    let mut s = settings_with(2, &r.device_id());
    s.devices.remove(&r.device_id());
    s.boxes.clear();
    send_settings(&r, 1, &sign_owner(s));
    eventually("the Box is off", || r.status() == BoxStatus::TurnedOff);
    eventually("the member list is empty", || r.members().is_empty());
    assert!(!dir.path().join(POS_TOKEN_FILE).exists());
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
