//! The call protocol: the main flows of the Rust API driven again through
//! `dispatch`, and the strictness of the request.

use std::sync::{Arc, Mutex};

use openbmt_core::say::{check, Quoted, Words};
use openbmt_owner::ItemOutcome;

use super::kit::*;
use super::wire::{frame, Rd, W};
use crate::say::{tone_byte, who_byte, TONES, WHOS};
use crate::{
    coin_bits, dice_bits, dice_face_counts, dispatch, envelope, error_response, parse_deposit_code,
    word_is_valid, word_suggestions, CeilingsView, Decision, EntropySource, FeeCeilingsView,
    KeyApp, KeyError, KeyNetwork, KeyStore, KeyWordCount, KitCheck, LineKind, Registry, SetupState,
    StoreError, MAX_APPS,
};
use crate::{words_for, Kind};

// ---- the ops, by number -----------------------------------------------------------

const OPEN: u16 = 1;
const CLOSE_APP: u16 = 2;
const CLOSE_SESSION: u16 = 3;
const STATE: u16 = 10;
const CREATE: u16 = 11;
const CREATE_APPROVER_ONLY: u16 = 12;
const KEY_WORDS: u16 = 13;
const KEY2_PUBLIC: u16 = 14;
const DEVICE_ROOT: u16 = 15;
const DEVICE_ROOT_WORDS: u16 = 16;
const ACCEPT_WALLET: u16 = 17;
const WALLET_VIEW: u16 = 18;
const PAIR_OFFER: u16 = 19;
const CHECK_KIT: u16 = 20;
const OPEN_SESSION: u16 = 30;
const SESSION_ID: u16 = 31;
const LINES: u16 = 32;
const DECIDE: u16 = 33;
const FINISH: u16 = 34;
const CANCEL: u16 = 35;
const SIGNER_STATUS: u16 = 40;
const RECEIVE_MARK: u16 = 41;
const ENROLL: u16 = 42;
const REMOVE: u16 = 43;
const SET_CEILINGS: u16 = 44;
const SET_FEE_CEILINGS: u16 = 45;
const RAISE_CHANGE: u16 = 46;
const RAISE_RECEIVE: u16 = 47;
const CORRECT_CLOCK: u16 = 48;
const COIN_BITS: u16 = 60;
const DICE_BITS: u16 = 61;
const DICE_FACE_COUNTS: u16 = 62;
const WORD_SUGGESTIONS: u16 = 63;
const WORD_IS_VALID: u16 = 64;
const PARSE_DEPOSIT: u16 = 65;

// ---- a client -----------------------------------------------------------------------

const OK: u8 = 0;
const REFUSED: u8 = 1;
const STORE: u8 = 2;
const STATE_ERR: u8 = 3;
const INTERNAL: u8 = 4;

/// What a response says: the result, or the status, rule and text.
type Reply = Result<Vec<u8>, (u8, String, String)>;

thread_local! {
    /// How many error responses this thread's `parse_response` has found
    /// words in that keep the rules.
    static CHECKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A strict cursor over the body of a response.
pub(crate) struct Cur<'a> {
    data: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, p: 0 }
    }

    fn take(&mut self, n: usize) -> &'a [u8] {
        let bytes = self.data.get(self.p..self.p + n).expect("bytes");
        self.p += n;
        bytes
    }

    fn u8(&mut self) -> u8 {
        *self.take(1).first().expect("a byte")
    }

    fn flag(&mut self) -> bool {
        match self.u8() {
            0 => false,
            1 => true,
            other => panic!("a flag of {other}"),
        }
    }

    fn str16(&mut self) -> String {
        let n = usize::from(u16::from_le_bytes(self.take(2).try_into().expect("2")));
        String::from_utf8(self.take(n).to_vec()).expect("UTF-8")
    }

    fn opt_str16(&mut self) -> Option<String> {
        self.flag().then(|| self.str16())
    }

    fn done(&self) -> bool {
        self.p == self.data.len()
    }
}

/// The words at the end of an error response, read strictly. The `code`
/// and the `from` of a quote are `&'static str` in `Words`, so the few a
/// test reads are leaked.
pub(crate) fn read_words(body: &[u8]) -> Words {
    let mut c = Cur::new(body);
    let tone = c.u8();
    let who = c.u8();
    let tone = TONES
        .iter()
        .find(|t| t.2 == tone)
        .unwrap_or_else(|| panic!("tone {tone}"))
        .0;
    let who = WHOS
        .iter()
        .find(|w| w.2 == who)
        .unwrap_or_else(|| panic!("who {who}"))
        .0;
    let heading = c.str16();
    let what = c.str16();
    let next = c.str16();
    let code = c.opt_str16();
    let at = c
        .flag()
        .then(|| u64::from_le_bytes(c.take(8).try_into().expect("8")));
    let quoted = c.flag().then(|| {
        let from = c.str16();
        let text = c.str16();
        (from, text)
    });
    let detail = c.opt_str16();
    assert!(c.done(), "bytes left over after the words");
    let mut w = Words::new(tone, heading, what, who, next);
    w.code = code.map(|c| &*Box::leak(c.into_boxed_str()));
    w.at = at;
    w.quoted = quoted.map(|(from, text)| Quoted {
        from: Box::leak(from.into_boxed_str()),
        text,
    });
    w.detail = detail;
    w
}

fn kind_of(status: u8) -> Kind {
    match status {
        REFUSED => Kind::Refused,
        STORE => Kind::Store,
        STATE_ERR => Kind::State,
        INTERNAL => Kind::Internal,
        other => panic!("status {other}"),
    }
}

fn parse_response(response: &[u8]) -> Reply {
    assert_eq!(response.get(..4), Some(b"OBM9".as_slice()), "magic");
    assert_eq!(response.get(4), Some(&1), "version");
    let status = *response.get(5).expect("a status");
    let body = response.get(6..).expect("a body");
    if status == OK {
        return Ok(body.to_vec());
    }
    assert!(status <= INTERNAL, "status {status}");
    let mut c = Cur::new(body);
    let rule = c.str16();
    let text = c.str16();
    let words = read_words(body.get(c.p..).expect("the words"));
    // Every error response carries words that keep the rules, and they are
    // the words of its kind, rule and text.
    assert_eq!(check(&words), Ok(()), "{rule}: {text}");
    assert_eq!(words, words_for(kind_of(status), &rule, &text));
    CHECKED.with(|n| n.set(n.get() + 1));
    Err((status, rule, text))
}

struct Client {
    reg: Registry,
    store: Arc<MemStore>,
}

impl Client {
    fn new(store: Arc<MemStore>) -> Self {
        Self {
            reg: Registry::new(),
            store,
        }
    }

    fn raw(&self, request: &[u8]) -> Reply {
        parse_response(&dispatch(&self.reg, self.store.clone(), request))
    }

    fn send(&self, op: u16, handle: u64, fields: W) -> Reply {
        self.raw(&frame(op, handle, &fields.0))
    }

    fn ok(&self, op: u16, handle: u64, fields: W) -> Vec<u8> {
        self.send(op, handle, fields)
            .unwrap_or_else(|e| panic!("op {op}: {e:?}"))
    }

    fn err(&self, op: u16, handle: u64, fields: W) -> (u8, String, String) {
        self.send(op, handle, fields)
            .expect_err(&format!("op {op} is refused"))
    }

    fn rule(&self, op: u16, handle: u64, fields: W) -> String {
        self.err(op, handle, fields).1
    }

    fn open(&self) -> u64 {
        Rd::new(&self.ok(OPEN, 0, W::new())).u64()
    }
}

fn state_of(c: &Client, app: u64) -> SetupState {
    Rd::new(&c.ok(STATE, app, W::new())).setup_state()
}

// ---- setup -------------------------------------------------------------------------

const ABANDON_12: [&str; 12] = [
    "abandon", "abandon", "abandon", "abandon", "abandon", "abandon", "abandon", "abandon",
    "abandon", "abandon", "abandon", "about",
];

fn sources() -> Vec<EntropySource> {
    vec![
        EntropySource::Platform,
        EntropySource::DiceHashed { rolls: rolls() },
        EntropySource::CoinFlips {
            heads: (0..600).map(|i| i % 4 == 1 || i % 4 == 2).collect(),
            words: KeyWordCount::Twelve,
        },
        EntropySource::DiceSkewCorrected {
            rolls: "12".repeat(300),
            words: KeyWordCount::TwentyFour,
        },
        EntropySource::TypedWords {
            words: ABANDON_12.iter().map(|w| (*w).to_string()).collect(),
        },
    ]
}

#[test]
fn setup_from_every_source_gives_what_the_api_gives() {
    for source in sources() {
        let c = Client::new(MemStore::new());
        let direct_store = MemStore::new();
        let direct = KeyApp::open(direct_store.clone()).expect("opens");
        let app = c.open();
        assert_eq!(state_of(&c, app), SetupState::Empty);
        assert_eq!(c.rule(KEY_WORDS, app, W::new()), "words-shown-once");
        assert_eq!(c.rule(KEY2_PUBLIC, app, W::new()), "empty");

        c.ok(
            CREATE,
            app,
            W::new().source(&source).network(KeyNetwork::Regtest),
        );
        direct
            .create_from_entropy(source.clone(), KeyNetwork::Regtest)
            .expect("direct");
        assert_eq!(c.store.saves(), direct_store.saves());
        assert_eq!(c.store.saved(), direct_store.saved());
        assert_eq!(state_of(&c, app), direct.state().expect("state"));

        let public = Rd::new(&c.ok(KEY2_PUBLIC, app, W::new())).opt(Rd::key2_public);
        assert_eq!(public, direct.key2_public().expect("asked"));
        let via = c.send(KEY_WORDS, app, W::new());
        match direct.key_words() {
            Ok(w) => assert_eq!(Rd::new(&via.expect("the words")).strings(), w),
            Err(_) => assert_eq!(via.expect_err("no words").1, "words-shown-once"),
        }
        assert_eq!(
            c.ok(DEVICE_ROOT, app, W::new()),
            W::new().bytes(&direct.device_root().expect("root")).0
        );
        let words = Rd::new(&c.ok(DEVICE_ROOT_WORDS, app, W::new())).strings();
        assert_eq!(words, direct.device_root_words().expect("words"));
    }
}

#[test]
fn a_typed_phrase_that_is_wrong_is_refused_through_the_protocol() {
    let c = Client::new(MemStore::new());
    let app = c.open();
    let typed = |words: &[&str]| EntropySource::TypedWords {
        words: words.iter().map(|w| (*w).to_string()).collect(),
    };
    let make =
        |s: &EntropySource| c.rule(CREATE, app, W::new().source(s).network(KeyNetwork::Regtest));
    assert_eq!(make(&typed(&["abandon"; 11])), "words-count");
    let mut unknown = ABANDON_12;
    unknown[3] = "zzzz";
    assert_eq!(make(&typed(&unknown)), "words-unknown");
    assert_eq!(make(&typed(&["abandon"; 12])), "words-checksum");
    assert_eq!(state_of(&c, app), SetupState::Empty);
    assert_eq!(c.store.saves(), 0);
}

#[test]
fn the_wallet_is_accepted_and_checked_through_the_protocol() {
    let c = Client::new(MemStore::new());
    let direct_store = MemStore::new();
    let direct = KeyApp::open(direct_store.clone()).expect("opens");
    let app = c.open();
    c.ok(
        CREATE,
        app,
        W::new()
            .source(&EntropySource::Platform)
            .network(KeyNetwork::Regtest),
    );
    direct
        .create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("direct");
    let public = Rd::new(&c.ok(KEY2_PUBLIC, app, W::new()))
        .opt(Rd::key2_public)
        .expect("key 2");
    let (wallet, _, key3) = wallet_from_public(&public);
    let setup = setup_for(&wallet, &key3);

    // Before the wallet, the ops that need it say so.
    for op in [WALLET_VIEW, PAIR_OFFER, SIGNER_STATUS] {
        assert_eq!(c.rule(op, app, W::new()), "not-ready");
    }
    let view = Rd::new(&c.ok(ACCEPT_WALLET, app, W::new().setup(&setup))).wallet_view();
    assert_eq!(view, direct.accept_wallet(setup.clone()).expect("direct"));
    assert_eq!(c.store.saved(), direct_store.saved());
    assert_eq!(
        Rd::new(&c.ok(WALLET_VIEW, app, W::new())).wallet_view(),
        view
    );
    assert_eq!(state_of(&c, app), SetupState::Ready { holds_key2: true });
    assert_eq!(
        c.ok(PAIR_OFFER, app, W::new()),
        W::new().bytes(&direct.pair_offer_bytes().expect("offer")).0
    );
    // Neither step again.
    assert_eq!(
        c.rule(ACCEPT_WALLET, app, W::new().setup(&setup)),
        "wallet-set"
    );
    assert_eq!(
        c.rule(
            CREATE,
            app,
            W::new()
                .source(&EntropySource::Platform)
                .network(KeyNetwork::Regtest)
        ),
        "wallet-set"
    );
    // A wallet that does not fit is refused by the rule crates.
    let mut bad = setup;
    bad.owner_root = vec![1; 31];
    let c2 = Client::new(MemStore::new());
    let app2 = c2.open();
    c2.ok(
        CREATE,
        app2,
        W::new()
            .source(&EntropySource::Platform)
            .network(KeyNetwork::Regtest),
    );
    assert_eq!(c2.rule(ACCEPT_WALLET, app2, W::new().setup(&bad)), "owner");

    // The kit.
    let own = wallet.canonical_string().to_string();
    let kit = |text: &str| Rd::new(&c.ok(CHECK_KIT, app, W::new().string(text))).kit_check();
    assert_eq!(kit(&own), KitCheck::Same);
    assert_eq!(kit(&format!("{own}\n")), KitCheck::Same);
    assert_eq!(
        kit(&own.replacen("sortedmulti(", "sortedmulti( ", 1)),
        direct
            .check_kit(own.replacen("sortedmulti(", "sortedmulti( ", 1))
            .expect("direct")
    );
    assert_eq!(
        c.rule(CHECK_KIT, app, W::new().string(&"x".repeat(9_000))),
        "kit-too-long"
    );
}

#[test]
fn an_approver_only_app_and_a_restart_through_the_protocol() {
    let c = Client::new(MemStore::new());
    let app = c.open();
    c.ok(CREATE_APPROVER_ONLY, app, W::new());
    assert_eq!(
        Rd::new(&c.ok(KEY2_PUBLIC, app, W::new())).opt(Rd::key2_public),
        None
    );
    assert_eq!(
        state_of(&c, app),
        SetupState::HasIdentity {
            holds_key2: false,
            words_waiting: false
        }
    );
    // A second app of the same registry restores what was stored.
    let second = c.open();
    assert!(second > app);
    assert_eq!(
        state_of(&c, second),
        SetupState::HasIdentity {
            holds_key2: false,
            words_waiting: false
        }
    );
}

// ---- sessions ----------------------------------------------------------------------

fn lines_of(c: &Client, session: u64) -> Vec<crate::LineView> {
    Rd::new(&c.ok(LINES, session, W::new())).lines()
}

#[test]
fn a_whole_session_runs_through_the_protocol() {
    let mut rig = rig();
    let request = session_request(&mut rig);
    let c = Client::new(rig.store.clone());
    let app = c.open();
    assert_eq!(state_of(&c, app), SetupState::Ready { holds_key2: true });

    let saves = rig.store.saves();
    let session = Rd::new(&c.ok(OPEN_SESSION, app, W::new().bytes(&request))).u64();
    assert!(session > app);
    // The lines are what the API's own session shows.
    let direct = rig.app.open_session(request.clone()).expect("opens");
    let lines = lines_of(&c, session);
    assert_eq!(lines, direct.lines().expect("lines"));
    assert_eq!(lines.len(), 4);
    assert_eq!(
        c.ok(SESSION_ID, session, W::new()),
        W::new().bytes(&direct.session_id().expect("id")).0
    );
    direct.cancel();

    // A second session of the app is refused while this one is open.
    let (status, rule, _) = c.err(OPEN_SESSION, app, W::new().bytes(&request));
    assert_eq!((status, rule.as_str()), (STATE_ERR, "session-open"));
    assert_eq!(c.rule(RAISE_CHANGE, app, W::new().u32(5)), "session-open");

    // A decline needs a reason; an item must exist.
    let spend = lines
        .iter()
        .find(|l| l.kind == LineKind::Spend)
        .expect("a spend")
        .item;
    assert_eq!(
        c.rule(
            DECIDE,
            session,
            W::new().u32(spend).decision(&Decision::Decline {
                reason: String::new()
            })
        ),
        "AS-30"
    );
    assert_eq!(
        c.rule(
            DECIDE,
            session,
            W::new().u32(999).decision(&Decision::Approve)
        ),
        "AS-30"
    );
    for l in &lines {
        let d = if l.item == spend {
            Decision::Decline {
                reason: "not this one".to_string(),
            }
        } else {
            Decision::Approve
        };
        c.ok(DECIDE, session, W::new().u32(l.item).decision(&d));
    }
    assert_eq!(rig.store.saves(), saves);
    let shown = lines_of(&c, session);
    assert!(shown.iter().all(|l| l.decision.is_some()));

    // The finish stores once, then returns the response.
    let response = Rd::new(&c.ok(FINISH, session, W::new())).bytes();
    assert_eq!(rig.store.saves(), saves + 1);
    assert!(lines_of(&c, session).is_empty());
    assert_eq!(c.err(FINISH, session, W::new()).0, REFUSED);
    let taken = rig
        .owner
        .take_response(&response, &mut rig.key1, &rig.key1_account, NOW, |_, _| {
            Ok::<(), String>(())
        })
        .expect("the owner takes the response");
    assert!(taken
        .outcomes
        .iter()
        .any(|(_, o)| matches!(o, ItemOutcome::Returned(r) if r.contains("not this one"))));
    let status = Rd::new(&c.ok(SIGNER_STATUS, app, W::new())).signer_status();
    // The spend was declined: nothing of it is in the log.
    assert_eq!(status.spend_log_count, 0);

    // The handle is closed; then it is gone.
    c.ok(CLOSE_SESSION, session, W::new());
    assert_eq!(c.rule(LINES, session, W::new()), "handle-unknown");
    assert_eq!(c.rule(CLOSE_SESSION, session, W::new()), "handle-unknown");
}

#[test]
fn cancel_releases_the_slot_and_a_new_session_ends_the_old_handle() {
    let mut rig = rig();
    let request = session_request(&mut rig);
    let c = Client::new(rig.store.clone());
    let app = c.open();
    let first = Rd::new(&c.ok(OPEN_SESSION, app, W::new().bytes(&request))).u64();
    c.ok(CANCEL, first, W::new());
    assert!(lines_of(&c, first).is_empty());
    assert!(c.send(FINISH, first, W::new()).is_err());
    // The slot is free: a new session opens, and the old handle is gone.
    let second = Rd::new(&c.ok(OPEN_SESSION, app, W::new().bytes(&request))).u64();
    assert!(second > first);
    assert_eq!(c.rule(LINES, first, W::new()), "handle-unknown");
    assert_eq!(lines_of(&c, second).len(), 4);
    // Closing the app closes its session.
    c.ok(CLOSE_APP, app, W::new());
    assert_eq!(c.rule(LINES, second, W::new()), "handle-unknown");
    assert_eq!(c.rule(STATE, app, W::new()), "handle-unknown");
    // Handles are not reused.
    let third = c.open();
    assert!(third > second);
}

#[test]
fn an_approver_only_app_has_no_signer_screen() {
    let c = Client::new(MemStore::new());
    let app = c.open();
    c.ok(CREATE_APPROVER_ONLY, app, W::new());
    let (_, slot1) = fixed_key(1, 1);
    let (_, slot2) = fixed_key(2, 2);
    let (_, slot3) = fixed_key(3, 3);
    let wallet = openbmt_core::wallet::WalletDescription::new(NET, [slot1, slot2, slot3.clone()])
        .expect("a wallet");
    c.ok(
        ACCEPT_WALLET,
        app,
        W::new().setup(&setup_for(&wallet, &slot3.xpub)),
    );
    assert_eq!(c.rule(SIGNER_STATUS, app, W::new()), "AS-36");
}

// ---- own screens -----------------------------------------------------------------------

fn address(n: u8) -> String {
    bitcoin::Address::from_script(&external(n), NET)
        .expect("an address")
        .to_string()
}

#[test]
fn the_own_screens_work_through_the_protocol() {
    let rig = rig();
    let c = Client::new(rig.store.clone());
    let app = c.open();
    let saves = rig.store.saves();
    let status = |c: &Client| Rd::new(&c.ok(SIGNER_STATUS, app, W::new())).signer_status();
    assert!(status(&c).allowlist.is_empty());

    c.ok(ENROLL, app, W::new().string(&address(5)).string("Exchange"));
    assert_eq!(rig.store.saves(), saves + 1);
    let after = status(&c);
    assert_eq!(after.allowlist.len(), 1);
    assert_eq!(after.allowlist[0].label, "Exchange");
    // The same state, read through the API from the stored bytes.
    let again = KeyApp::open(rig.store.clone()).expect("restarts");
    assert_eq!(again.signer_status().expect("status"), after);
    assert_eq!(
        c.rule(ENROLL, app, W::new().string(&address(5)).string("Again")),
        "SR-30"
    );
    c.ok(REMOVE, app, W::new().string(&address(5)));
    assert!(status(&c).allowlist.is_empty());

    let other = CeilingsView {
        per_tx: 50_000,
        per_day: 150_000,
        per_week: 400_000,
    };
    let allowlisted = CeilingsView {
        per_tx: 1_000_000,
        per_day: 2_000_000,
        per_week: 5_000_000,
    };
    c.ok(
        SET_CEILINGS,
        app,
        W::new().ceilings(&other).ceilings(&allowlisted),
    );
    let fee = FeeCeilingsView {
        sat_per_vb: 80,
        absolute_sats: 100_000,
    };
    c.ok(
        SET_FEE_CEILINGS,
        app,
        W::new().u64(fee.sat_per_vb).u64(fee.absolute_sats),
    );
    c.ok(RAISE_CHANGE, app, W::new().u32(10));
    c.ok(RAISE_RECEIVE, app, W::new().u32(30));
    let s = status(&c);
    assert_eq!(s.ceilings_other, other);
    assert_eq!(s.ceilings_allowlisted, allowlisted);
    assert_eq!(s.fee_ceilings, fee);
    assert_eq!((s.change_mark, s.receive_mark), (10, 30));
    assert_eq!(Rd::new(&c.ok(RECEIVE_MARK, app, W::new())).u32(), 30);
    assert_eq!(c.rule(RAISE_RECEIVE, app, W::new().u32(30)), "SR-70");
    // A clock that is not behind changes nothing.
    c.ok(CORRECT_CLOCK, app, W::new());
    let again = KeyApp::open(rig.store.clone()).expect("restarts");
    assert_eq!(again.signer_status().expect("status"), status(&c));
}

#[test]
fn a_store_that_fails_changes_nothing_through_the_protocol() {
    let rig = rig();
    let c = Client::new(rig.store.clone());
    let app = c.open();
    let before = c.ok(SIGNER_STATUS, app, W::new());
    let bytes = rig.store.saved().expect("stored");
    rig.store.fail_saves(true);
    let (status, rule, text) = c.err(ENROLL, app, W::new().string(&address(5)).string("Exchange"));
    assert_eq!((status, rule.as_str()), (STORE, "AS-34"));
    assert!(text.contains("disk full"));
    assert_eq!(c.err(RAISE_RECEIVE, app, W::new().u32(50)).0, STORE);
    assert_eq!(c.ok(SIGNER_STATUS, app, W::new()), before);
    assert_eq!(rig.store.saved().expect("stored"), bytes);
    rig.store.fail_saves(false);
    c.ok(RAISE_RECEIVE, app, W::new().u32(50));

    // The same during setup: nothing is made.
    let fresh = Client::new(MemStore::new());
    let a = fresh.open();
    fresh.store.fail_saves(true);
    let (status, rule, _) = fresh.err(
        CREATE,
        a,
        W::new()
            .source(&EntropySource::Platform)
            .network(KeyNetwork::Regtest),
    );
    assert_eq!((status, rule.as_str()), (STORE, "AS-34"));
    assert_eq!(state_of(&fresh, a), SetupState::Empty);
    assert!(fresh.store.saved().is_none());
}

#[test]
fn a_session_whose_store_fails_finishes_later() {
    let mut rig = rig();
    let request = session_request(&mut rig);
    let before = rig.store.saved().expect("stored");
    let c = Client::new(rig.store.clone());
    let app = c.open();
    let session = Rd::new(&c.ok(OPEN_SESSION, app, W::new().bytes(&request))).u64();
    for l in lines_of(&c, session) {
        c.ok(
            DECIDE,
            session,
            W::new().u32(l.item).decision(&Decision::Approve),
        );
    }
    rig.store.fail_saves(true);
    let (status, rule, _) = c.err(FINISH, session, W::new());
    assert_eq!((status, rule.as_str()), (STORE, "AS-34"));
    assert_eq!(rig.store.saved().expect("stored"), before);
    rig.store.fail_saves(false);
    assert!(!c.ok(FINISH, session, W::new()).is_empty());
    assert_ne!(rig.store.saved().expect("stored"), before);
}

// ---- free functions ----------------------------------------------------------------------

#[test]
fn the_free_functions_work_through_the_protocol() {
    let c = Client::new(MemStore::new());
    let heads: Vec<bool> = (0..100).map(|i| i % 4 == 1 || i % 4 == 2).collect();
    let mut w = W::new().u32(100);
    for h in &heads {
        w = w.bool(*h);
    }
    let got = Rd::new(&c.ok(COIN_BITS, 0, w.words(KeyWordCount::Twelve))).progress();
    assert_eq!(got, coin_bits(heads, KeyWordCount::Twelve));

    let rolls = "12".repeat(300);
    let got = Rd::new(&c.ok(
        DICE_BITS,
        0,
        W::new().string(&rolls).words(KeyWordCount::TwentyFour),
    ))
    .progress();
    assert_eq!(
        got,
        dice_bits(rolls, KeyWordCount::TwentyFour).expect("bits")
    );
    assert_eq!(
        c.rule(
            DICE_BITS,
            0,
            W::new().string("7").words(KeyWordCount::Twelve)
        ),
        "entropy"
    );
    assert_eq!(
        Rd::new(&c.ok(DICE_FACE_COUNTS, 0, W::new().string("112"))).u32s(),
        dice_face_counts("112".to_string()).expect("counts")
    );
    assert_eq!(
        Rd::new(&c.ok(WORD_SUGGESTIONS, 0, W::new().string("aba"))).strings(),
        word_suggestions("aba".to_string())
    );
    for (word, valid) in [("abandon", true), ("zzz", false)] {
        assert_eq!(
            Rd::new(&c.ok(WORD_IS_VALID, 0, W::new().string(word))).bool(),
            valid
        );
        assert_eq!(word_is_valid(word.to_string()), valid);
    }
    let bech = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
    let text = format!("bitcoin:{bech}?amount=0.5&label=Shop&pj=x");
    let got = Rd::new(&c.ok(
        PARSE_DEPOSIT,
        0,
        W::new().string(&text).network(KeyNetwork::Mainnet),
    ))
    .deposit();
    assert_eq!(
        got,
        parse_deposit_code(text, KeyNetwork::Mainnet).expect("parses")
    );
    assert_eq!(
        c.rule(
            PARSE_DEPOSIT,
            0,
            W::new().string("nonsense").network(KeyNetwork::Mainnet)
        ),
        parse_deposit_code("nonsense".to_string(), KeyNetwork::Mainnet)
            .expect_err("refused")
            .rule()
    );
    // These work on no object.
    assert_eq!(
        c.rule(WORD_IS_VALID, 1, W::new().string("abandon")),
        "handle-unexpected"
    );
}

// ---- a panic stays inside --------------------------------------------------------------------

/// A store that panics where the test says.
struct Panicky {
    on_random: bool,
    on_load: bool,
}

impl KeyStore for Panicky {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        if self.on_load {
            panic!("a panic in load");
        }
        Ok(None)
    }

    fn save(&self, _bytes: Vec<u8>) -> Result<(), StoreError> {
        Ok(())
    }

    fn random(&self, _n: u32) -> Result<Vec<u8>, StoreError> {
        if self.on_random {
            panic!("a panic in random");
        }
        Ok(vec![7; 32])
    }

    fn now(&self) -> u64 {
        NOW
    }
}

#[test]
fn a_panic_becomes_status_4_and_poisons_the_app() {
    let reg = Registry::new();
    let store: Arc<dyn KeyStore> = Arc::new(Panicky {
        on_random: true,
        on_load: false,
    });
    let send = |op: u16, handle: u64, fields: W| {
        parse_response(&dispatch(
            &reg,
            store.clone(),
            &frame(op, handle, &fields.0),
        ))
    };
    let app = Rd::new(&send(OPEN, 0, W::new()).expect("opens")).u64();
    let (status, rule, text) = send(
        CREATE,
        app,
        W::new()
            .source(&EntropySource::Platform)
            .network(KeyNetwork::Regtest),
    )
    .expect_err("the panic is caught");
    assert_eq!((status, rule.as_str()), (INTERNAL, "internal"));
    assert!(text.contains("a panic in random"));
    // Every later call of the app returns the same kind of error.
    for op in [STATE, SIGNER_STATUS, RECEIVE_MARK] {
        assert_eq!(send(op, app, W::new()).expect_err("poisoned").0, INTERNAL);
    }
    // The registry is fine: a panic in load while opening is caught too.
    let loud: Arc<dyn KeyStore> = Arc::new(Panicky {
        on_random: false,
        on_load: true,
    });
    let reply = parse_response(&dispatch(&reg, loud, &frame(OPEN, 0, &[])));
    assert_eq!(reply.expect_err("caught").0, INTERNAL);
    let fine = Rd::new(&send(OPEN, 0, W::new()).expect("opens")).u64();
    assert_eq!(
        Rd::new(&send(STATE, fine, W::new()).expect("state")).setup_state(),
        SetupState::Empty
    );
}

/// A store that calls back into the registry while a call runs.
struct Reenter {
    reg: Arc<Registry>,
    seen: Mutex<Vec<Reply>>,
}

impl KeyStore for Reenter {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        let inner = dispatch(
            &self.reg,
            Arc::new(Panicky {
                on_random: false,
                on_load: false,
            }),
            &frame(OPEN, 0, &[]),
        );
        self.seen.lock().expect("lock").push(parse_response(&inner));
        Ok(None)
    }

    fn save(&self, _bytes: Vec<u8>) -> Result<(), StoreError> {
        Ok(())
    }

    fn random(&self, _n: u32) -> Result<Vec<u8>, StoreError> {
        Ok(vec![7; 32])
    }

    fn now(&self) -> u64 {
        NOW
    }
}

#[test]
fn a_call_from_inside_a_call_is_refused_not_waited_for() {
    let reg = Arc::new(Registry::new());
    let store = Arc::new(Reenter {
        reg: reg.clone(),
        seen: Mutex::new(Vec::new()),
    });
    let reply = parse_response(&dispatch(&reg, store.clone(), &frame(OPEN, 0, &[])));
    assert!(reply.is_ok());
    let seen = store.seen.lock().expect("lock");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].as_ref().expect_err("refused").1, "busy");
}

// ---- strictness --------------------------------------------------------------------------------

#[test]
fn the_frame_is_checked() {
    let c = Client::new(MemStore::new());
    let rule = |r: Reply| r.expect_err("refused").1;
    assert_eq!(rule(c.raw(&[])), "request-malformed");
    assert_eq!(rule(c.raw(b"OBM8")), "request-malformed");
    let mut bad = frame(OPEN, 0, &[]);
    bad[0] = b'X';
    assert_eq!(rule(c.raw(&bad)), "request-magic");
    let mut bad = frame(OPEN, 0, &[]);
    bad[4] = 2;
    assert_eq!(rule(c.raw(&bad)), "request-version");
    // An op that is not in the table.
    for op in [0, 4, 9, 21, 36, 49, 66, 999, u16::MAX] {
        let (status, rule, _) = c.err(op, 0, W::new());
        assert_eq!((status, rule.as_str()), (REFUSED, "op-unknown"), "op {op}");
    }
    // Bytes left over.
    assert_eq!(rule(c.raw(&frame(OPEN, 0, &[0]))), "request-malformed");
    let app = c.open();
    assert_eq!(rule(c.raw(&frame(STATE, app, &[0]))), "request-malformed");
    // Too long: over 1 MiB.
    let mut huge = frame(WORD_IS_VALID, 0, &[]);
    huge.resize(1 << 20, 0);
    assert_eq!(rule(c.raw(&huge)), "request-malformed");
    huge.push(0);
    assert_eq!(rule(c.raw(&huge)), "request-too-long");
}

#[test]
fn lengths_that_are_too_big_are_refused_without_allocating_them() {
    let c = Client::new(MemStore::new());
    let app = c.open();
    // A string longer than the request.
    assert_eq!(
        c.rule(CHECK_KIT, app, W::new().u32(u32::MAX)),
        "request-malformed"
    );
    assert_eq!(
        c.rule(CHECK_KIT, app, W::new().u32(100).u8(b'a')),
        "request-malformed"
    );
    // A list longer than the bytes that are left.
    assert_eq!(
        c.rule(COIN_BITS, 0, W::new().u32(u32::MAX).u8(1)),
        "request-malformed"
    );
    // A flag, a tag and a bool that are not defined.
    assert_eq!(
        c.rule(CREATE, app, W::new().u8(9).network(KeyNetwork::Regtest)),
        "request-malformed"
    );
    assert_eq!(
        c.rule(CREATE, app, W::new().u8(0).u8(9)),
        "request-malformed"
    );
    assert_eq!(
        c.rule(ENROLL, app, W::new().string("a").u32(5)),
        "request-malformed"
    );
    assert_eq!(
        c.rule(
            COIN_BITS,
            0,
            W::new().u32(1).u8(2).words(KeyWordCount::Twelve)
        ),
        "request-malformed"
    );
    // Text that is not UTF-8.
    assert_eq!(
        c.rule(CHECK_KIT, app, W::new().u32(1).u8(0xff)),
        "request-malformed"
    );
}

#[test]
fn handles_are_checked() {
    let c = Client::new(MemStore::new());
    // No object has this handle.
    for handle in [0, 1, 77, u64::MAX] {
        assert_eq!(c.rule(STATE, handle, W::new()), "handle-unknown");
        assert_eq!(c.rule(LINES, handle, W::new()), "handle-unknown");
        assert_eq!(c.rule(CLOSE_APP, handle, W::new()), "handle-unknown");
    }
    // `open` works on no object.
    assert_eq!(c.rule(OPEN, 1, W::new()), "handle-unexpected");
    let app = c.open();
    assert_eq!(app, 1);
    // An app handle is not a session handle, and the other way round.
    assert_eq!(c.rule(LINES, app, W::new()), "handle-unknown");
    // Closing, then using, then closing again.
    c.ok(CLOSE_APP, app, W::new());
    assert_eq!(c.rule(STATE, app, W::new()), "handle-unknown");
    assert_eq!(c.rule(CLOSE_APP, app, W::new()), "handle-unknown");
    // A closed handle is not given out again.
    let next = c.open();
    assert_eq!(next, 2);
}

#[test]
fn at_most_four_apps_are_open() {
    let c = Client::new(MemStore::new());
    let handles: Vec<u64> = (0..MAX_APPS).map(|_| c.open()).collect();
    let (status, rule, _) = c.err(OPEN, 0, W::new());
    assert_eq!((status, rule.as_str()), (STATE_ERR, "app-limit"));
    let first = *handles.first().expect("one");
    c.ok(CLOSE_APP, first, W::new());
    assert!(c.open() > *handles.last().expect("one"));
}

#[test]
fn an_error_frame_is_well_formed_for_every_kind() {
    for (e, status) in [
        (
            KeyError::Refused {
                rule: "r".to_string(),
                text: "t".to_string(),
            },
            REFUSED,
        ),
        (
            KeyError::Store {
                rule: "r".to_string(),
                text: "t".to_string(),
            },
            STORE,
        ),
        (
            KeyError::State {
                rule: "r".to_string(),
                text: "t".to_string(),
            },
            STATE_ERR,
        ),
        (
            KeyError::Internal {
                rule: "r".to_string(),
                text: "t".to_string(),
            },
            INTERNAL,
        ),
    ] {
        let got = parse_response(&error_response(&e)).expect_err("an error");
        assert_eq!(got, (status, "r".to_string(), "t".to_string()));
    }
    // A text longer than 65,535 bytes is cut at a character boundary.
    let long = KeyError::Refused {
        rule: "r".to_string(),
        text: "\u{e9}".repeat(40_000),
    };
    let got = parse_response(&error_response(&long)).expect_err("an error");
    assert!(got.2.len() <= 65_535 && got.2.len() > 65_000);
    assert!(got.2.chars().all(|c| c == '\u{e9}'));
}

#[test]
fn an_error_frame_carries_the_words_after_the_rule_and_the_text() {
    let e = KeyError::State {
        rule: "empty".to_string(),
        text: "no key has been made yet".to_string(),
    };
    let frame = error_response(&e);
    // Magic, version, status 3, then rule and text.
    let mut expect = b"OBM9".to_vec();
    expect.extend_from_slice(&[1, STATE_ERR]);
    for text in ["empty", "no key has been made yet"] {
        expect.extend_from_slice(&(text.len() as u16).to_le_bytes());
        expect.extend_from_slice(text.as_bytes());
    }
    assert_eq!(frame.get(..expect.len()), Some(expect.as_slice()));
    let words = frame.get(expect.len()..).expect("the words");
    // Tone Info is 0 and who KeyHolder is 5; then heading, what, next.
    assert_eq!(&words[..2], &[0, 5]);
    let mut c = Cur::new(&words[2..]);
    assert_eq!(c.str16(), "No key yet");
    assert_eq!(c.str16(), "This app has no key yet.");
    assert_eq!(
        c.str16(),
        "Make the key, or type the words of an existing key."
    );
    // No code, no time, no quoted text, a detail with the text.
    assert!(!c.flag());
    assert!(!c.flag());
    assert!(!c.flag());
    assert_eq!(c.opt_str16().as_deref(), Some("no key has been made yet"));
    assert!(c.done());
}

#[test]
fn every_field_of_the_words_is_written_and_read_back() {
    use crate::call::put_words;
    use openbmt_core::say::{Tone, Who};
    let full = Words::new(
        Tone::Stop,
        "A heading",
        "What happened.",
        Who::Owner,
        "What to do.",
    )
    .code("AS-20")
    .at(1_700_000_000)
    .quoted(Quoted::new("The owner's phone says", "Refund for Ann"))
    .detail("record 7");
    let mut out = Vec::new();
    put_words(&mut out, &full);
    assert_eq!(read_words(&out), full);
    for (tone, _, byte) in TONES {
        assert_eq!(tone_byte(tone), byte);
    }
    for (who, _, byte) in WHOS {
        assert_eq!(who_byte(who), byte);
    }
    let bare = Words::new(Tone::Good, "Done", "It is done.", Who::Nobody, "Nothing.");
    let mut out = Vec::new();
    put_words(&mut out, &bare);
    assert_eq!(read_words(&out), bare);
}

#[test]
fn every_error_the_crate_can_name_travels_with_words_that_keep_the_rules() {
    for (kind, rule, text) in crate::say::samples() {
        let (rule, text) = (rule.to_string(), text.to_string());
        let e = match kind {
            Kind::Refused => KeyError::Refused { rule, text },
            Kind::Store => KeyError::Store { rule, text },
            Kind::State => KeyError::State { rule, text },
            Kind::Internal => KeyError::Internal { rule, text },
        };
        // `parse_response` checks the words and compares them with
        // `words_for`.
        parse_response(&error_response(&e)).expect_err("an error");
    }
}

/// A Key app that is ready, with a session open, in a fresh registry:
/// app handle 1, session handle 2.
fn world(stored: &[u8], request: &[u8]) -> Client {
    let store = MemStore::new();
    store.set_saved(Some(stored.to_vec()));
    let c = Client::new(store);
    assert_eq!(c.open(), 1);
    assert_eq!(
        Rd::new(&c.ok(OPEN_SESSION, 1, W::new().bytes(request))).u64(),
        2
    );
    c
}

#[test]
fn every_cut_and_every_flipped_bit_of_a_valid_request_is_handled() {
    let mut rig = rig();
    let request = session_request(&mut rig);
    let stored = rig.store.saved().expect("stored");
    let (wallet, _, key3) = {
        // The wallet of the rig, for a setup request.
        let public = rig.app.key2_public().expect("asked").expect("key 2");
        wallet_from_public(&public)
    };
    let valid: Vec<(&str, Vec<u8>)> = vec![
        ("state", frame(STATE, 1, &[])),
        ("close_session", frame(CLOSE_SESSION, 2, &[])),
        ("lines", frame(LINES, 2, &[])),
        (
            "decide",
            frame(DECIDE, 2, &W::new().u32(0).decision(&Decision::Approve).0),
        ),
        (
            "decline",
            frame(
                DECIDE,
                2,
                &W::new()
                    .u32(1)
                    .decision(&Decision::Decline {
                        reason: "no".to_string(),
                    })
                    .0,
            ),
        ),
        (
            "create",
            frame(
                CREATE,
                1,
                &W::new()
                    .source(&EntropySource::DiceHashed { rolls: rolls() })
                    .network(KeyNetwork::Regtest)
                    .0,
            ),
        ),
        (
            "typed",
            frame(
                CREATE,
                1,
                &W::new()
                    .source(&EntropySource::TypedWords {
                        words: ABANDON_12.iter().map(|w| (*w).to_string()).collect(),
                    })
                    .network(KeyNetwork::Regtest)
                    .0,
            ),
        ),
        (
            "accept_wallet",
            frame(
                ACCEPT_WALLET,
                1,
                &W::new().setup(&setup_for(&wallet, &key3)).0,
            ),
        ),
        (
            "check_kit",
            frame(CHECK_KIT, 1, &W::new().string("wsh(x)#abcd").0),
        ),
        (
            "enroll",
            frame(ENROLL, 1, &W::new().string(&address(5)).string("Shop").0),
        ),
        (
            "set_ceilings",
            frame(
                SET_CEILINGS,
                1,
                &W::new()
                    .ceilings(&CeilingsView {
                        per_tx: 1,
                        per_day: 2,
                        per_week: 3,
                    })
                    .ceilings(&CeilingsView {
                        per_tx: 4,
                        per_day: 5,
                        per_week: 6,
                    })
                    .0,
            ),
        ),
        (
            "raise_receive",
            frame(RAISE_RECEIVE, 1, &W::new().u32(99).0),
        ),
        (
            "coin_bits",
            frame(
                COIN_BITS,
                0,
                &W::new()
                    .u32(3)
                    .bool(true)
                    .bool(false)
                    .bool(true)
                    .words(KeyWordCount::Twelve)
                    .0,
            ),
        ),
        (
            "deposit",
            frame(
                PARSE_DEPOSIT,
                0,
                &W::new()
                    .string("bitcoin:bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq?amount=1")
                    .network(KeyNetwork::Mainnet)
                    .0,
            ),
        ),
        (
            "open_session",
            frame(OPEN_SESSION, 1, &W::new().bytes(&request).0),
        ),
    ];
    let shared = world(&stored, &request);
    let mut flips = 0usize;
    for (name, bytes) in &valid {
        // The request as it is works or is refused by a rule, never a panic.
        let w = world(&stored, &request);
        let whole = w.raw(bytes);
        assert!(
            !matches!(&whole, Err((s, _, _)) if *s == INTERNAL),
            "{name}: {whole:?}"
        );
        // Every cut is refused.
        for n in 0..bytes.len() {
            let cut = bytes.get(..n).expect("a prefix");
            let reply = shared.raw(cut);
            let (status, _, _) = reply.expect_err(&format!("{name} cut at {n}"));
            assert_eq!(status, REFUSED, "{name} cut at {n}");
        }
        // Every flipped bit. A flip can turn the request into another that
        // works (a close, say), so the world is made again after any reply
        // that is not a refusal. Long requests flip every bit of the header
        // and the start of the fields, and then every bit of one byte in 13.
        let mut w = world(&stored, &request);
        for i in 0..bytes.len() {
            if bytes.len() > 400 && i > 64 && i % 13 != 0 {
                continue;
            }
            for bit in 0..8 {
                let mut flipped = bytes.clone();
                flipped[i] ^= 1 << bit;
                let reply = w.raw(&flipped);
                if let Err((status, rule, text)) = &reply {
                    assert_ne!(
                        *status, INTERNAL,
                        "{name} byte {i} bit {bit}: {rule}: {text}"
                    );
                }
                if let Some(saved) = w.store.saved() {
                    envelope::decode(&saved).expect("the stored bytes always decode");
                }
                if reply.is_ok() {
                    w = world(&stored, &request);
                }
                flips += 1;
            }
        }
    }
    assert!(flips > 2_000, "{flips}");
    // Every error response of the corpus carried words that keep the rules
    // (`parse_response` checks each one).
    let checked = CHECKED.with(std::cell::Cell::get);
    assert!(checked > 2_000, "{checked}");
}
