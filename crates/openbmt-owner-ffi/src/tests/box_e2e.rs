//! Connecting a Box from end to end (`docs/OWNER-APP-RULES.md` OA-40 to
//! OA-48, `docs/SESSION-RULES.md` AS-78 to AS-87, `docs/BOX-RULES.md` BX-10
//! to BX-17): the Key app makes the invitation, the Box takes it, the owner
//! phone fetches the claim, the Key app checks it and welcomes the Box in an
//! approval session, the owner phone hands the Box the welcome, and the
//! owner's link then passes a sealed message through the Box. Every call on
//! the phones' sides goes through the libraries the apps use: `openbmt-key-ffi`
//! and `dispatch`. The Box is a `BoxRunner` on the loopback, or, for
//! `scripts/box-e2e.sh`, a real `openbmt-box` process (the ignored test).

use std::net::SocketAddr;
use std::sync::Arc;

use openbmt_box::BoxRunner;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_key_ffi::{Decision, KeyApp};
use openbmt_link::{open, os_random, seal, Client, ClientConfig, Kind, Role, Route, ServerConfig};
use openbmt_net_io::SystemClock;

use super::ceremony::{answer_of, key_app_with_key2, started};
use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{frame, Rd, W};
use crate::ceremony::{finish, FinishArgs};
use crate::{dispatch, OwnerNetwork};

/// What a run of the pairing found, for the test and for the script.
struct Report {
    /// The four words the owner phone showed for the Box.
    words: Vec<String>,
    /// The Box's key.
    box_key: Vec<u8>,
    /// What the Box said to the owner's message, as the owner read it.
    message: String,
}

/// The call `op` with `fields`, which must be refused, with words that keep
/// the rules of SC-12.
fn refused(ph: &Phones, op: u16, fields: &[u8]) -> super::wire::Fault {
    let response = ph.rig.call(op, ph.handle, fields);
    let fault = Rd::error(&response).expect("refused");
    check_wire_words(&fault.words).unwrap_or_else(|why| panic!("{why}: {:?}", fault.words));
    fault
}

fn fetch_fields(address: &str) -> Vec<u8> {
    W::new().string(address).string("").0
}

fn started_box(dir_name: &str) -> (BoxRunner, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("{dir_name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let runner = BoxRunner::start(
        &dir,
        "127.0.0.1:0".parse::<SocketAddr>().expect("an address"),
        Arc::new(SystemClock),
        ServerConfig::default(),
    )
    .expect("the Box starts");
    (runner, dir)
}

/// The two phones: a Key app made by its ceremony with the owner phone, and
/// the owner's node started from the finished setup.
pub(super) struct Phones {
    app: Arc<KeyApp>,
    rig: Rig,
    handle: u64,
    owner: Identity,
}

fn phones() -> Phones {
    let app = key_app_with_key2();
    let answer = answer_of(&app);
    let (_, built) = started(&answer);
    app.accept_wallet_setup(built.wallet_setup.clone())
        .expect("accepted");
    app.confirm_wallet().expect("the words match");
    let done = finish(FinishArgs {
        record: built.record.clone(),
        refund_depth: 6,
        birthday: 0,
    })
    .expect("finished");
    let fields = crate::ceremony::open(done.record.clone()).expect("opens");
    let rig = Rig::new();
    let mut config = start_config(rig.dir().to_str().expect("UTF-8"));
    config.network = OwnerNetwork::Regtest;
    config.owner_secret = fields.owner_secret.clone();
    config.approver_root = fields.approver_root.clone();
    config.wallet = fields.wallet.clone();
    config.key1_account = fields.key1_account.clone();
    config.key3_xpub = fields.key3_xpub.clone();
    config.batch_min_count = fields.batch_min_count;
    config.refund_depth = fields.refund_depth;
    config.birthday = fields.birthday;
    let response = dispatch(rig.registry(), &frame(1, 0, &W::new().start(&config).0));
    let handle = Rd::ok(&response).u64();
    let owner =
        Identity::from_secret_bytes(&fields.owner_secret.clone().try_into().expect("32 bytes"))
            .expect("the owner's key");
    Phones {
        app,
        rig,
        handle,
        owner,
    }
}

impl Phones {
    /// Settings that list a counter, signed by the owner, queued (the shop
    /// already has a counter, so the Box has someone to talk to).
    fn queue_counter_settings(&self, counter: &DeviceId) {
        let mut s = Settings::defaults(100_000);
        s.version = 1;
        s.devices.insert(*counter);
        s.counters.insert(*counter, 10_000);
        let signed = SignedSettings {
            owner_sig: s.sign(&self.owner),
            approver_sig: None,
            settings: s,
        };
        Rd::ok(
            &self
                .rig
                .call(46, self.handle, &W::new().bytes(&signed.to_bytes()).0),
        )
        .u8();
    }

    /// `box_fetch_claim` until the Box has the invitation (the real Box
    /// takes it from a console, a moment after it was typed).
    fn fetch_claim(&self, address: &str) -> (Vec<u8>, Vec<u8>, bool) {
        let mut last = None;
        for _ in 0..40 {
            let response = self
                .rig
                .call(104, self.handle, &W::new().string(address).string("").0);
            if let Some(e) = Rd::error(&response) {
                last = Some(e.rule.clone());
                std::thread::sleep(std::time::Duration::from_millis(250));
                continue;
            }
            let mut r = Rd::ok(&response);
            let found = (r.bytes(), r.bytes(), r.bool());
            r.done();
            return found;
        }
        panic!("no claim from the Box: {last:?}");
    }
}

/// Everything up to the hand-over: the Box has an invitation, its claim is
/// queued, the Key app approved it, and its welcome waits.
struct Staged {
    ph: Phones,
    counter: Identity,
    box_key: Vec<u8>,
    words: Vec<String>,
}

/// Stage the Box at `address`. `enter` types the invitation into the Box.
fn stage(address: &str, enter: &mut dyn FnMut(&str)) -> Staged {
    let ph = phones();
    let counter = id(0x31);
    ph.queue_counter_settings(&counter.device_id());

    // The Key app makes the invitation; it is typed into the Box.
    let invitation = ph.app.make_invitation().expect("an invitation");
    assert_eq!(invitation.text.split('-').count(), 6);
    enter(&invitation.text);

    // The owner phone fetches the claim and queues it with the settings
    // that add the Box.
    let (claim, box_key, needs_rate) = ph.fetch_claim(address);
    assert!(!needs_rate, "settings are queued already");
    let response = ph
        .rig
        .call(105, ph.handle, &W::new().bytes(&claim).u64(0).0);
    let mut r = Rd::ok(&response);
    let (claim_id, settings_id, version) = (r.u32(), r.u32(), r.u32());
    let queued_key = r.bytes();
    let words = r.list(Rd::string);
    r.done();
    assert_eq!(queued_key, box_key);
    assert_eq!(words.len(), 4);
    assert!(claim_id != settings_id && version == 2);

    // The approval session: the Key app shows "Box found" with the same
    // four words, checks the claim, approves and welcomes.
    let now = T0 + 10;
    let response = ph.rig.call(30, ph.handle, &W::new().u64(now).u64(1_000).0);
    let mut r = Rd::ok(&response);
    let (_, request, _) = r.built();
    r.done();
    let session = ph.app.open_session(request).expect("a session");
    let mut shown = Vec::new();
    let items: Vec<u32> = session
        .lines()
        .expect("lines")
        .iter()
        .map(|l| l.item)
        .collect();
    for item in items {
        // The lines are read again each time: the claim can be approved
        // only after the settings that add the Box were.
        let line = session
            .lines()
            .expect("lines")
            .into_iter()
            .find(|l| l.item == item)
            .expect("a line");
        if let openbmt_key_ffi::LineDetail::BoxClaim { detail } = &line.detail {
            shown = detail.words.clone().expect("the Box's words");
        }
        assert!(line.can_approve, "{:?} {:?}", line.title, line.failure);
        session.decide(item, Decision::Approve).expect("decided");
    }
    assert_eq!(shown, words, "the Key app and the owner phone agree");
    let answer = session.finish().expect("the response");
    let response = ph
        .rig
        .call(32, ph.handle, &W::new().u64(now + 5).bytes(&answer).0);
    Rd::ok(&response);
    assert!(
        Rd::ok(&ph.rig.call(106, ph.handle, &[]))
            .list(Rd::bytes)
            .len()
            == 1,
        "one Box waits for its welcome"
    );

    Staged {
        ph,
        counter,
        box_key,
        words,
    }
}

/// Pair the Box at `address`. `enter` types the invitation into the Box.
fn pair_a_box(address: &str, enter: &mut dyn FnMut(&str)) -> Report {
    let Staged {
        ph,
        counter,
        box_key,
        words,
    } = stage(address, enter);
    // The owner phone hands the Box its welcome.
    let response = ph.rig.call(
        107,
        ph.handle,
        &W::new().string(address).string("").bytes(&box_key).0,
    );
    let mut r = Rd::ok(&response);
    let (delivered_key, counters, settings_version) = (r.bytes(), r.list(Rd::bytes), r.u32());
    r.done();
    assert_eq!(delivered_key, box_key);
    assert_eq!(counters, vec![counter.device_id().to_bytes().to_vec()]);
    assert_eq!(settings_version, 2);
    assert!(
        Rd::ok(&ph.rig.call(106, ph.handle, &[]))
            .list(Rd::bytes)
            .is_empty(),
        "nothing waits after the Box took it"
    );

    // The owner's link: a counter's message goes through the Box and the
    // owner reads it.
    let key = DeviceId::from_bytes(&box_key.clone().try_into().expect("32 bytes")).expect("a key");
    let addr: SocketAddr = address.parse().expect("an address");
    let mut client = Client::connect(&Route::Direct(addr), Some(&key), &ClientConfig::default())
        .expect("the owner's link");
    client.make_mailbox(&ph.owner).expect("the owner's mailbox");
    let mut rng = |b: &mut [u8; 32]| os_random(b);
    let sealed = seal(
        &counter,
        Role::Counter,
        &ph.owner.device_id(),
        Role::Owner,
        1,
        Kind::CheckIn,
        b"hello through the Box",
        &mut rng,
    )
    .expect("sealed");
    Client::connect(&Route::Direct(addr), Some(&key), &ClientConfig::default())
        .expect("the counter's link")
        .put(&sealed)
        .expect("put");
    let got = client.fetch(&ph.owner, 0).expect("fetched");
    assert_eq!(got.items.len(), 1);
    let opened = open(
        &ph.owner,
        Role::Owner,
        &got.items[0].1,
        |d| (*d == counter.device_id()).then_some(Role::Counter),
        |_| 0,
    )
    .expect("opened");
    assert_eq!(opened.sender, counter.device_id());
    Rd::ok(&ph.rig.call(2, ph.handle, &[])).pump();
    Report {
        words,
        box_key,
        message: String::from_utf8_lossy(&opened.content).to_string(),
    }
}

#[test]
fn oa_41_to_oa_43_a_box_is_paired_through_the_phones_libraries() {
    let dir = std::env::temp_dir().join(format!("openbmt-owner-ffi-box-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let runner = BoxRunner::start(
        &dir,
        "127.0.0.1:0".parse::<SocketAddr>().expect("an address"),
        Arc::new(SystemClock),
        ServerConfig::default(),
    )
    .expect("the Box starts");
    let address = runner.local_addr().to_string();
    let report = pair_a_box(&address, &mut |text| {
        runner.enter_invitation(text).expect("the Box takes it");
    });
    assert_eq!(report.message, "hello through the Box");
    assert_eq!(report.box_key, runner.device_id().to_bytes().to_vec());
    assert_eq!(
        report.words,
        runner.fingerprint_words().to_vec(),
        "the Box shows the words the phones showed"
    );
    assert_eq!(runner.status().text(), "paired");
    runner.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// For `scripts/box-e2e.sh` (step 18): a real `openbmt-box` process at
/// `OPENBMT_BOX_E2E_ADDR`, whose console is the fifo
/// `OPENBMT_BOX_E2E_CONSOLE`. Prints what the script compares.
#[test]
#[ignore = "needs a Box process: run by scripts/box-e2e.sh"]
fn oa_41_to_oa_43_a_real_box_process_is_paired() {
    let address = std::env::var("OPENBMT_BOX_E2E_ADDR").expect("OPENBMT_BOX_E2E_ADDR");
    let console = std::env::var("OPENBMT_BOX_E2E_CONSOLE").expect("OPENBMT_BOX_E2E_CONSOLE");
    let report = pair_a_box(&address, &mut |text| {
        use std::io::Write;
        let mut fifo = std::fs::OpenOptions::new()
            .write(true)
            .open(&console)
            .expect("the Box's console");
        writeln!(fifo, "invite {text}").expect("typed");
    });
    println!("phones: box words: {}", report.words.join(" "));
    println!(
        "phones: box key: {}",
        report
            .box_key
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    println!("phones: message: {}", report.message);
    println!("phones: paired");
}

#[test]
fn oa_41_an_address_that_is_not_one_is_refused_with_words() {
    let ph = phones();
    for bad in [
        "",
        "box.local:8080",
        "192.168.1.20",
        "192.168.1.20:0",
        "0.0.0.0:80",
        "abc.onion:80",
        "http://192.168.1.20:80",
    ] {
        let fault = refused(&ph, 104, &fetch_fields(bad));
        assert_eq!(fault.rule, "box-address", "{bad:?}");
    }
    // A v3 onion name with a proxy that cannot be read is the address's
    // fault too.
    let onion = format!("{}.onion:80", "a".repeat(56));
    let fault = refused(&ph, 104, &W::new().string(&onion).string("not a proxy").0);
    assert_eq!(fault.rule, "box-address");
}

#[test]
fn oa_41_a_box_that_is_not_there_is_not_reached() {
    let ph = phones();
    // A port nothing listens on.
    let free = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = free.local_addr().expect("an address").to_string();
    drop(free);
    let fault = refused(&ph, 104, &fetch_fields(&address));
    assert_eq!(fault.rule, "box-unreachable");
}

#[test]
fn oa_41_something_that_is_not_a_box_is_not_a_box() {
    use std::io::Write;
    let ph = phones();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("an address").to_string();
    let junk = std::thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            let _ = s.write_all(&[0x42; 64]);
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    });
    let fault = refused(&ph, 104, &fetch_fields(&address));
    assert_eq!(fault.rule, "box-not-a-box");
    let _ = junk.join();
}

#[test]
fn oa_41_a_box_with_no_invitation_offers_no_claim() {
    let ph = phones();
    let (runner, dir) = started_box("openbmt-owner-ffi-box-none");
    let fault = refused(&ph, 104, &fetch_fields(&runner.local_addr().to_string()));
    assert_eq!(fault.rule, "box-no-invitation");
    runner.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn oa_41_a_box_the_shop_already_has_is_said_to_be_paired() {
    let (runner, dir) = started_box("openbmt-owner-ffi-box-paired");
    let address = runner.local_addr().to_string();
    let staged = stage(&address, &mut |t| {
        runner.enter_invitation(t).expect("taken")
    });
    // Its claim is queued and approved, and its welcome waits. The Box
    // still holds its invitation, so it offers its claim again, and the
    // shop knows it already.
    let fault = refused(&staged.ph, 104, &fetch_fields(&address));
    assert_eq!(fault.rule, "box-paired");
    runner.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn oa_43_a_box_that_refuses_the_welcome_changes_nothing_and_says_what_to_do() {
    let (runner, dir) = started_box("openbmt-owner-ffi-box-refuse");
    let address = runner.local_addr().to_string();
    let staged = stage(&address, &mut |t| {
        runner.enter_invitation(t).expect("taken")
    });
    let ph = &staged.ph;
    // Another invitation replaces the one the welcome was made for.
    let other = ph.app.make_invitation().expect("another");
    runner.enter_invitation(&other.text).expect("taken");
    let deliver = W::new()
        .string(&address)
        .string("")
        .bytes(&staged.box_key)
        .0;
    let fault = refused(ph, 107, &deliver);
    assert_eq!(fault.rule, "box-welcome-refused");
    assert!(fault.words.next.contains("new invitation"));
    // Nothing was taken out of the outbox: the approval stays queued.
    assert_eq!(
        Rd::ok(&ph.rig.call(106, ph.handle, &[]))
            .list(Rd::bytes)
            .len(),
        1
    );
    // A Box that is not the one asked for is told apart.
    let (stranger, other_dir) = started_box("openbmt-owner-ffi-box-stranger");
    let fault = refused(
        ph,
        107,
        &W::new()
            .string(&stranger.local_addr().to_string())
            .string("")
            .bytes(&staged.box_key)
            .0,
    );
    assert_eq!(fault.rule, "box-wrong-key");
    // A key that is not a waiting Box's is refused by the queue's own words.
    let fault = refused(
        ph,
        107,
        &W::new().string(&address).string("").bytes(&[9; 32]).0,
    );
    assert!(!fault.rule.is_empty());
    stranger.stop();
    runner.stop();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other_dir);
}

#[test]
fn oa_46_without_a_key_app_the_owner_phone_makes_the_invitation_and_checks_the_claim() {
    let ph = phones();
    let response = ph.rig.call(108, 0, &W::new().u64(T0).0);
    let mut r = Rd::ok(&response);
    let (text, expires) = (r.string(), r.u64());
    r.done();
    assert_eq!(text.split('-').count(), 6);
    assert_eq!(expires, T0 + 1800);
    let (runner, dir) = started_box("openbmt-owner-ffi-box-own");
    runner.enter_invitation(&text).expect("taken");
    let claim = Client::connect(
        &Route::Direct(runner.local_addr()),
        None,
        &ClientConfig::default(),
    )
    .expect("connects")
    .claim()
    .expect("a claim");
    // The claim answers the invitation: the Box's key and words come back.
    let response = ph.rig.call(109, 0, &W::new().string(&text).bytes(&claim).0);
    let mut r = Rd::ok(&response);
    let (key, words) = (r.bytes(), r.list(Rd::string));
    r.done();
    assert_eq!(key, runner.device_id().to_bytes().to_vec());
    assert_eq!(words, runner.fingerprint_words().to_vec());
    // Another invitation does not.
    let other = Rd::ok(&ph.rig.call(108, 0, &W::new().u64(T0).0)).string();
    let fault = refused_zero(&ph, 109, &W::new().string(&other).bytes(&claim).0);
    assert_eq!(fault.rule, "box-claim-mismatch");
    // Text that is not an invitation, and bytes that are not a claim.
    let fault = refused_zero(
        &ph,
        109,
        &W::new().string("not-an-invitation").bytes(&claim).0,
    );
    assert_eq!(fault.rule, "box-own-invitation");
    let fault = refused_zero(&ph, 109, &W::new().string(&text).bytes(b"junk").0);
    assert_eq!(fault.rule, "box-not-a-box");
    runner.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same for an op that takes no node.
fn refused_zero(ph: &Phones, op: u16, fields: &[u8]) -> super::wire::Fault {
    let response = ph.rig.call(op, 0, fields);
    let fault = Rd::error(&response).expect("refused");
    check_wire_words(&fault.words).unwrap_or_else(|why| panic!("{why}: {:?}", fault.words));
    fault
}

#[test]
fn oa_47_a_box_that_was_not_delivered_is_listed_and_forgotten() {
    let (runner, dir) = started_box("openbmt-owner-ffi-box-forget");
    let address = runner.local_addr().to_string();
    let staged = stage(&address, &mut |t| {
        runner.enter_invitation(t).expect("taken")
    });
    let ph = &staged.ph;
    // The Box is pending with its words and its welcome ready.
    let pending = |ph: &Phones| {
        let response = ph.rig.call(110, ph.handle, &[]);
        let mut r = Rd::ok(&response);
        let rows = r.list(|r| (r.bytes(), r.list(Rd::string), r.bool()));
        r.done();
        rows
    };
    let rows = pending(ph);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, staged.box_key);
    assert_eq!(rows[0].1, staged.words);
    assert!(rows[0].2);
    // Forgetting takes the claim, the settings and the welcome out at once.
    Rd::ok(
        &ph.rig
            .call(111, ph.handle, &W::new().bytes(&staged.box_key).0),
    );
    assert!(pending(ph).is_empty());
    assert!(Rd::ok(&ph.rig.call(106, ph.handle, &[]))
        .list(Rd::bytes)
        .is_empty());
    // Nothing is left to forget, said in words; a key of the wrong length
    // is refused too.
    let fault = refused(ph, 111, &W::new().bytes(&staged.box_key).0);
    assert_eq!(fault.rule, "AS-87");
    assert!(!fault.words.next.is_empty());
    let fault = refused(ph, 111, &W::new().bytes(&[1, 2, 3]).0);
    assert_eq!(fault.rule, "id-length");
    runner.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
