//! The call protocol: one function, [`dispatch`], that takes a framed
//! request and returns a framed response. Every call of the Key app's API is
//! one op of it, so a platform binding needs one entry point and no
//! generated code.
//!
//! ```text
//! request   "OBM8" | version 1 | op u16 | handle u64 | fields...
//! response  "OBM9" | version 1 | status u8 | ...
//!   status 0  ok: the result of the op
//!   status 1  refused  (KeyError::Refused)
//!   status 2  store    (KeyError::Store)
//!   status 3  state    (KeyError::State)
//!   status 4  internal (KeyError::Internal, a panic was caught)
//!   status 1 to 4 carry rule and text, then the words (see below)
//! ```
//!
//! After rule and text an error response carries the words the person is
//! shown (`docs/SCREEN-RULES.md`), in this order and with every string a u16
//! length and UTF-8: tone u8, who u8, heading, what, next, code (a flag
//! byte, then a string), at (a flag byte, then a u64), quoted (a flag byte,
//! then from and text) and detail (a flag byte, then a string). The numbers
//! of tone and who are in `bindings/PROTOCOL.md`.
//!
//! Integers are little-endian; the field encodings are in
//! [`codec`](crate::codec). The request is read completely before anything
//! is done, and it is refused when it is longer than 1 MiB, has bytes left
//! over after the last field, or holds a length, flag or tag that is not
//! defined. An op number that is not in the table is refused with the rule
//! `op-unknown`.
//!
//! The op table is the one generated into `bindings/PROTOCOL.md`; op numbers
//! are never reused.
//!
//! - 1 `open`, 2 `close_app`, 3 `close_session`: handles.
//! - 10 to 20: setup (`state`, `create_from_entropy`, `create_approver_only`,
//!   `key_words`, `key2_public`, `device_root`, `device_root_words`,
//!   `accept_wallet`, `wallet_view`, `pair_offer_bytes`, `check_kit`).
//! - 30 to 35: one approval session (`open_session`, `session_id`, `lines`,
//!   `decide`, `finish`, `cancel`).
//! - 40 to 48: the Key app's own screens.
//! - 50 to 52: pairing a Box (`make_invitation`, `open_invitation`,
//!   `cancel_invitation`; `docs/SIGNER-RULES.md` SR-80).
//! - 60 to 65: free functions (entropy progress, word picker, deposit code).
//! - 90 to 93: the lock of the stored state (`lock_status`, `set_pin`,
//!   `unlock`, `change_pin`; `docs/SIGNER-RULES.md` SR-71 to SR-74).
//! - 80 to 85: the key ceremony (`take_key3`, `key3_view`, `public_answer`,
//!   `accept_wallet_setup`, `confirm_wallet`, `discard_wallet`;
//!   `docs/SIGNER-RULES.md` SR-64 to SR-69).
//!
//! The handle field names what the op works on: an app handle (from `open`)
//! or a session handle (from `open_session`), or must be 0. Handles start at
//! 1 and are never reused in a process. A [`Registry`] holds at most 4 open
//! apps and, for each, the handle of its last session: opening a session ends
//! the handle of the one before it.
//!
//! The store is handed in with every call and is used only during it: the
//! app's side keeps nothing between two calls. One call runs at a time; a
//! call made while another is running (or from inside the store) is refused
//! with the rule `busy`.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};

use openbmt_core::say::Words;

use crate::app::{Key2Public, KeyApp, LockStatus, SetupState, SlotView, WalletSetup, WalletView};
use crate::boxinvite::InvitationView;
use crate::ceremony::Key3View;
use crate::codec::{Define, Get, Put, Reader, TypeDef, MAX_REQUEST};
use crate::deposit::{parse_deposit_code, DepositCode};
use crate::error::{internal, refused_by, wrong_state, KeyError};
use crate::keys::{
    coin_bits, dice_bits, dice_face_counts, BitProgress, EntropySource, KeyNetwork, KeyWordCount,
};
use crate::kitcheck::KitCheck;
use crate::say::{tone_byte, who_byte, TONES, WHOS};
use crate::screen::{AllowlistView, CeilingsView, FeeCeilingsView, SignerStatus};
use crate::session::SessionHandle;
use crate::store::KeyStore;
use crate::views::{
    BatchDetail, BoxClaimDetail, CeilingsRemaining, CheckStatus, CheckView, Decision, Direction,
    HealthDetail, LineDetail, LineKind, LineView, OutputKindView, OutputView, OwnerLabelKind,
    OwnerLabelView, PairingDetail, SettingChangeView, SettingsDetail, SpendDetail, SpendReviewView,
};
use crate::words::{word_is_valid, word_suggestions};

/// The first four bytes of a request.
pub const REQUEST_MAGIC: [u8; 4] = *b"OBM8";
/// The first four bytes of a response.
pub const RESPONSE_MAGIC: [u8; 4] = *b"OBM9";
/// The version of both frames.
pub const VERSION: u8 = 1;

/// The most apps a registry holds open.
pub const MAX_APPS: usize = 4;

/// What a response says about the call.
const STATUS_OK: u8 = 0;
const STATUS_REFUSED: u8 = 1;
const STATUS_STORE: u8 = 2;
const STATUS_STATE: u8 = 3;
const STATUS_INTERNAL: u8 = 4;

/// What the handle of a request names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handle {
    /// No object: the field must be 0.
    Zero,
    /// An app handle.
    App,
    /// A session handle.
    Session,
}

impl Handle {
    fn name(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::App => "app",
            Self::Session => "session",
        }
    }
}

struct SessionEntry {
    app_handle: u64,
    app: Arc<KeyApp>,
    session: Arc<SessionHandle>,
}

struct RegState {
    next: u64,
    apps: BTreeMap<u64, Arc<KeyApp>>,
    sessions: BTreeMap<u64, SessionEntry>,
}

/// The open apps and sessions of a process, by handle. [`dispatch`] takes
/// one by reference; a platform binding keeps one in a static.
pub struct Registry {
    /// Held for the whole of a call, so that a store is used by one call
    /// at a time.
    serial: Mutex<()>,
    state: Mutex<RegState>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

fn unknown_handle() -> KeyError {
    refused_by(
        "handle-unknown",
        "no object has this handle: it was never opened or has been closed",
    )
}

impl Registry {
    /// An empty registry. The first handle given out is 1.
    pub fn new() -> Self {
        Self {
            serial: Mutex::new(()),
            state: Mutex::new(RegState {
                next: 1,
                apps: BTreeMap::new(),
                sessions: BTreeMap::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, RegState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn new_handle(state: &mut RegState) -> Result<u64, KeyError> {
        let handle = state.next;
        state.next = handle
            .checked_add(1)
            .ok_or_else(|| wrong_state("handles-used", "every handle of this process is used"))?;
        Ok(handle)
    }

    fn check_room(&self) -> Result<(), KeyError> {
        if self.lock().apps.len() >= MAX_APPS {
            return Err(wrong_state(
                "app-limit",
                "four apps are open: close one before opening another",
            ));
        }
        Ok(())
    }

    fn add_app(&self, app: Arc<KeyApp>) -> Result<u64, KeyError> {
        let mut state = self.lock();
        if state.apps.len() >= MAX_APPS {
            return Err(wrong_state(
                "app-limit",
                "four apps are open: close one before opening another",
            ));
        }
        let handle = Self::new_handle(&mut state)?;
        state.apps.insert(handle, app);
        Ok(handle)
    }

    fn add_session(
        &self,
        app_handle: u64,
        app: &Arc<KeyApp>,
        session: Arc<SessionHandle>,
    ) -> Result<u64, KeyError> {
        let (handle, ended) = {
            let mut state = self.lock();
            let handle = Self::new_handle(&mut state)?;
            let old: Vec<u64> = state
                .sessions
                .iter()
                .filter(|(_, e)| e.app_handle == app_handle)
                .map(|(h, _)| *h)
                .collect();
            let ended: Vec<SessionEntry> = old
                .iter()
                .filter_map(|h| state.sessions.remove(h))
                .collect();
            state.sessions.insert(
                handle,
                SessionEntry {
                    app_handle,
                    app: Arc::clone(app),
                    session,
                },
            );
            (handle, ended)
        };
        // Dropped here, with the registry unlocked.
        drop(ended);
        Ok(handle)
    }

    fn remove_app(&self, handle: u64) -> Result<(), KeyError> {
        let (app, ended) = {
            let mut state = self.lock();
            let app = state.apps.remove(&handle).ok_or_else(unknown_handle)?;
            let gone: Vec<u64> = state
                .sessions
                .iter()
                .filter(|(_, e)| e.app_handle == handle)
                .map(|(h, _)| *h)
                .collect();
            let ended: Vec<SessionEntry> = gone
                .iter()
                .filter_map(|h| state.sessions.remove(h))
                .collect();
            (app, ended)
        };
        drop(ended);
        drop(app);
        Ok(())
    }

    fn remove_session(&self, handle: u64) -> Result<(), KeyError> {
        let entry = self
            .lock()
            .sessions
            .remove(&handle)
            .ok_or_else(unknown_handle)?;
        drop(entry);
        Ok(())
    }

    fn resolve<'a>(
        &'a self,
        kind: Handle,
        handle: u64,
        store: Arc<dyn KeyStore>,
    ) -> Result<Ctx<'a>, KeyError> {
        let mut ctx = Ctx {
            registry: self,
            store,
            handle,
            app: None,
            session: None,
        };
        match kind {
            Handle::Zero => {
                if handle != 0 {
                    return Err(refused_by(
                        "handle-unexpected",
                        "this op works on no object: its handle must be 0",
                    ));
                }
            }
            Handle::App => {
                ctx.app = Some(
                    self.lock()
                        .apps
                        .get(&handle)
                        .cloned()
                        .ok_or_else(unknown_handle)?,
                );
            }
            Handle::Session => {
                let state = self.lock();
                let entry = state.sessions.get(&handle).ok_or_else(unknown_handle)?;
                ctx.app = Some(Arc::clone(&entry.app));
                ctx.session = Some(Arc::clone(&entry.session));
            }
        }
        Ok(ctx)
    }
}

/// What an op works with.
pub(crate) struct Ctx<'a> {
    registry: &'a Registry,
    store: Arc<dyn KeyStore>,
    handle: u64,
    app: Option<Arc<KeyApp>>,
    session: Option<Arc<SessionHandle>>,
}

impl Ctx<'_> {
    fn app(&self) -> Result<&Arc<KeyApp>, KeyError> {
        self.app
            .as_ref()
            .ok_or_else(|| internal("the op has no app"))
    }

    fn session(&self) -> Result<&Arc<SessionHandle>, KeyError> {
        self.session
            .as_ref()
            .ok_or_else(|| internal("the op has no session"))
    }
}

type Run = Box<dyn Fn(&Ctx<'_>, &mut Reader<'_>) -> Result<Vec<u8>, KeyError> + Send + Sync>;

/// One op of the table.
pub(crate) struct Op {
    pub num: u16,
    pub name: &'static str,
    pub handle: Handle,
    pub args: String,
    pub result: String,
    run: Run,
}

/// An op whose arguments are `A` and whose result is `R`. The table, the
/// decoding and the description are all made from these two types.
fn op<A: Get + 'static, R: Put + 'static>(
    num: u16,
    name: &'static str,
    handle: Handle,
    f: fn(&Ctx<'_>, A) -> Result<R, KeyError>,
) -> Op {
    Op {
        num,
        name,
        handle,
        args: A::shape(),
        result: R::shape(),
        run: Box::new(move |ctx, r| {
            let args = A::get(r)?;
            r.end()?;
            let mut out = Vec::new();
            f(ctx, args)?.put(&mut out);
            Ok(out)
        }),
    }
}

/// The table of ops.
pub(crate) fn ops() -> &'static [Op] {
    static OPS: OnceLock<Vec<Op>> = OnceLock::new();
    OPS.get_or_init(build_ops)
}

fn build_ops() -> Vec<Op> {
    use Handle::{App, Session, Zero};
    vec![
        op(1, "open", Zero, |c, ()| {
            c.registry.check_room()?;
            let app = KeyApp::open(Arc::clone(&c.store))?;
            app.detach();
            c.registry.add_app(app)
        }),
        op(2, "close_app", App, |c, ()| c.registry.remove_app(c.handle)),
        op(3, "close_session", Session, |c, ()| {
            c.session()?.cancel();
            c.registry.remove_session(c.handle)
        }),
        op(10, "state", App, |c, ()| c.app()?.state()),
        op(
            11,
            "create_from_entropy",
            App,
            |c, (source, network): (EntropySource, KeyNetwork)| {
                c.app()?.create_from_entropy(source, network)
            },
        ),
        op(12, "create_approver_only", App, |c, ()| {
            c.app()?.create_approver_only()
        }),
        op(13, "key_words", App, |c, ()| c.app()?.key_words()),
        op(14, "key2_public", App, |c, ()| c.app()?.key2_public()),
        op(15, "device_root", App, |c, ()| c.app()?.device_root()),
        op(16, "device_root_words", App, |c, ()| {
            c.app()?.device_root_words()
        }),
        op(17, "accept_wallet", App, |c, setup: WalletSetup| {
            c.app()?.accept_wallet(setup)
        }),
        op(18, "wallet_view", App, |c, ()| c.app()?.wallet_view()),
        op(19, "pair_offer_bytes", App, |c, ()| {
            c.app()?.pair_offer_bytes()
        }),
        op(20, "check_kit", App, |c, scanned: String| {
            c.app()?.check_kit(scanned)
        }),
        op(30, "open_session", App, |c, request: Vec<u8>| {
            let app = c.app()?;
            let session = app.open_session(request)?;
            c.registry.add_session(c.handle, app, session)
        }),
        op(31, "session_id", Session, |c, ()| c.session()?.session_id()),
        op(32, "lines", Session, |c, ()| c.session()?.lines()),
        op(
            33,
            "decide",
            Session,
            |c, (item, decision): (u32, Decision)| c.session()?.decide(item, decision),
        ),
        op(34, "finish", Session, |c, ()| c.session()?.finish()),
        op(35, "cancel", Session, |c, ()| {
            c.session()?.cancel();
            Ok(())
        }),
        op(40, "signer_status", App, |c, ()| c.app()?.signer_status()),
        op(41, "receive_mark", App, |c, ()| c.app()?.receive_mark()),
        op(
            42,
            "enroll_destination",
            App,
            |c, (address, label): (String, String)| c.app()?.enroll_destination(address, label),
        ),
        op(43, "remove_destination", App, |c, address: String| {
            c.app()?.remove_destination(address)
        }),
        op(
            44,
            "set_ceilings",
            App,
            |c, (other, allowlisted): (CeilingsView, CeilingsView)| {
                c.app()?.set_ceilings(other, allowlisted)
            },
        ),
        op(45, "set_fee_ceilings", App, |c, fee: FeeCeilingsView| {
            c.app()?.set_fee_ceilings(fee)
        }),
        op(46, "raise_change_mark", App, |c, mark: u32| {
            c.app()?.raise_change_mark(mark)
        }),
        op(47, "raise_receive_mark", App, |c, mark: u32| {
            c.app()?.raise_receive_mark(mark)
        }),
        op(48, "correct_clock", App, |c, ()| c.app()?.correct_clock()),
        op(50, "make_invitation", App, |c, ()| {
            c.app()?.make_invitation()
        }),
        op(51, "open_invitation", App, |c, ()| {
            c.app()?.open_invitation()
        }),
        op(52, "cancel_invitation", App, |c, ()| {
            c.app()?.cancel_invitation()
        }),
        op(
            60,
            "coin_bits",
            Zero,
            |_, (heads, words): (Vec<bool>, KeyWordCount)| Ok(coin_bits(heads, words)),
        ),
        op(
            61,
            "dice_bits",
            Zero,
            |_, (rolls, words): (String, KeyWordCount)| dice_bits(rolls, words),
        ),
        op(62, "dice_face_counts", Zero, |_, rolls: String| {
            dice_face_counts(rolls)
        }),
        op(63, "word_suggestions", Zero, |_, prefix: String| {
            Ok(word_suggestions(prefix))
        }),
        op(64, "word_is_valid", Zero, |_, word: String| {
            Ok(word_is_valid(word))
        }),
        op(
            65,
            "parse_deposit_code",
            Zero,
            |_, (text, network): (String, KeyNetwork)| parse_deposit_code(text, network),
        ),
        op(80, "take_key3", App, |c, text: String| {
            c.app()?.take_key3(text)
        }),
        op(81, "key3_view", App, |c, ()| c.app()?.key3_view()),
        op(82, "public_answer", App, |c, ()| c.app()?.public_answer()),
        op(83, "accept_wallet_setup", App, |c, bytes: Vec<u8>| {
            c.app()?.accept_wallet_setup(bytes)
        }),
        op(84, "confirm_wallet", App, |c, ()| c.app()?.confirm_wallet()),
        op(85, "discard_wallet", App, |c, ()| c.app()?.discard_wallet()),
        op(90, "lock_status", App, |c, ()| c.app()?.lock_status()),
        op(91, "set_pin", App, |c, pin: String| c.app()?.set_pin(pin)),
        op(92, "unlock", App, |c, pin: String| c.app()?.unlock(pin)),
        op(
            93,
            "change_pin",
            App,
            |c, (current, new): (String, String)| c.app()?.change_pin(current, new),
        ),
    ]
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "a panic".to_string()
    }
}

/// Make the call: the header, the op, the handle, the store, the op.
fn call(
    registry: &Registry,
    store: Arc<dyn KeyStore>,
    request: &[u8],
) -> Result<Vec<u8>, KeyError> {
    let _serial = match registry.serial.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(guard)) => guard.into_inner(),
        Err(TryLockError::WouldBlock) => {
            return Err(refused_by(
                "busy",
                "another call is running: calls are made one at a time",
            ))
        }
    };
    if request.len() > MAX_REQUEST {
        return Err(refused_by(
            "request-too-long",
            format!("a request is at most {MAX_REQUEST} bytes"),
        ));
    }
    let mut r = Reader::new(request);
    if r.take(4)? != REQUEST_MAGIC {
        return Err(refused_by(
            "request-magic",
            "the request does not start OBM8",
        ));
    }
    if r.u8()? != VERSION {
        return Err(refused_by(
            "request-version",
            "the request version is not 1",
        ));
    }
    let num = r.u16()?;
    let handle = r.u64()?;
    let op = ops()
        .iter()
        .find(|o| o.num == num)
        .ok_or_else(|| refused_by("op-unknown", format!("{num} is not the number of an op")))?;
    let ctx = registry.resolve(op.handle, handle, Arc::clone(&store))?;
    if let Some(app) = &ctx.app {
        app.attach(store);
    }
    let result = catch_unwind(AssertUnwindSafe(|| (op.run)(&ctx, &mut r)));
    if let Some(app) = &ctx.app {
        app.detach();
    }
    match result {
        Ok(result) => result,
        Err(p) => Err(internal(format!("caught a panic: {}", panic_text(&*p)))),
    }
}

/// Run one request. The response is always a frame: a result, or an error
/// with its rule and text. A panic anywhere in the call becomes status 4.
///
/// `store` is used only during the call. A request that is not read
/// completely, or names an op or a handle that does not exist, changes
/// nothing.
pub fn dispatch(registry: &Registry, store: Arc<dyn KeyStore>, request: &[u8]) -> Vec<u8> {
    match catch_unwind(AssertUnwindSafe(|| call(registry, store, request))) {
        Ok(Ok(payload)) => {
            let mut out = Vec::with_capacity(payload.len().saturating_add(8));
            out.extend_from_slice(&RESPONSE_MAGIC);
            out.push(VERSION);
            out.push(STATUS_OK);
            out.extend_from_slice(&payload);
            out
        }
        Ok(Err(e)) => error_response(&e),
        Err(p) => error_response(&internal(format!("caught a panic: {}", panic_text(&*p)))),
    }
}

fn put_str16(out: &mut Vec<u8>, s: &str) {
    let mut n = s.len().min(usize::from(u16::MAX));
    while !s.is_char_boundary(n) {
        n = n.saturating_sub(1);
    }
    out.extend_from_slice(&u16::try_from(n).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(s.as_bytes().get(..n).unwrap_or_default());
}

fn put_flagged_str16(out: &mut Vec<u8>, s: Option<&str>) {
    match s {
        Some(s) => {
            out.push(1);
            put_str16(out, s);
        }
        None => out.push(0),
    }
}

/// The words of an error as the response frame carries them, after the rule
/// and the text.
pub(crate) fn put_words(out: &mut Vec<u8>, w: &Words) {
    out.push(tone_byte(w.tone));
    out.push(who_byte(w.who));
    put_str16(out, &w.heading);
    put_str16(out, &w.what);
    put_str16(out, &w.next);
    put_flagged_str16(out, w.code);
    match w.at {
        Some(at) => {
            out.push(1);
            out.extend_from_slice(&at.to_le_bytes());
        }
        None => out.push(0),
    }
    match &w.quoted {
        Some(q) => {
            out.push(1);
            put_str16(out, q.from);
            put_str16(out, &q.text);
        }
        None => out.push(0),
    }
    put_flagged_str16(out, w.detail.as_deref());
}

/// The response frame of an error. For a binding that fails before it can
/// call [`dispatch`].
pub fn error_response(e: &KeyError) -> Vec<u8> {
    let status = match e {
        KeyError::Refused { .. } => STATUS_REFUSED,
        KeyError::Store { .. } => STATUS_STORE,
        KeyError::State { .. } => STATUS_STATE,
        KeyError::Internal { .. } => STATUS_INTERNAL,
    };
    let mut out = Vec::new();
    out.extend_from_slice(&RESPONSE_MAGIC);
    out.push(VERSION);
    out.push(status);
    put_str16(&mut out, e.rule());
    // Panic payloads are arbitrary program data and may contain secrets.
    // Do not copy one into the response frame sent to the platform app.
    let text = if matches!(e, KeyError::Internal { text, .. } if text.starts_with("caught a panic: "))
    {
        "caught a panic: [redacted]"
    } else {
        e.text()
    };
    put_str16(&mut out, text);
    put_words(&mut out, &e.words());
    out
}

// ---- the description of the protocol, generated from the code above ----------

fn definitions() -> Vec<TypeDef> {
    vec![
        KeyNetwork::define(),
        KeyWordCount::define(),
        EntropySource::define(),
        SetupState::define(),
        LockStatus::define(),
        Key2Public::define(),
        Key3View::define(),
        SlotView::define(),
        WalletSetup::define(),
        WalletView::define(),
        KitCheck::define(),
        Decision::define(),
        LineKind::define(),
        LineView::define(),
        LineDetail::define(),
        SpendDetail::define(),
        SpendReviewView::define(),
        OutputView::define(),
        OutputKindView::define(),
        OwnerLabelView::define(),
        OwnerLabelKind::define(),
        CeilingsRemaining::define(),
        BatchDetail::define(),
        CheckView::define(),
        CheckStatus::define(),
        SettingsDetail::define(),
        SettingChangeView::define(),
        Direction::define(),
        PairingDetail::define(),
        HealthDetail::define(),
        BoxClaimDetail::define(),
        InvitationView::define(),
        SignerStatus::define(),
        AllowlistView::define(),
        CeilingsView::define(),
        FeeCeilingsView::define(),
        BitProgress::define(),
        DepositCode::define(),
    ]
}

/// The tag bytes of every enum: `(type, variant, tag)`.
#[cfg(test)]
pub(crate) fn enum_tags() -> Vec<(&'static str, &'static str, u8)> {
    definitions()
        .into_iter()
        .flat_map(|d| {
            d.tags
                .into_iter()
                .map(move |(variant, tag)| (d.name, variant, tag))
        })
        .collect()
}

/// The names of the types that `shape` mentions.
#[cfg(test)]
pub(crate) fn type_names(shape: &str) -> Vec<String> {
    shape
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|w| w.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
        .map(str::to_string)
        .collect()
}

/// The text of `bindings/PROTOCOL.md`: the frames, the op table and the
/// layout of every type, made from the tables the encoder uses.
pub fn protocol_description() -> String {
    let mut s = String::new();
    s.push_str(
        "# The Key app call protocol\n\
         \n\
         Generated by a test of `openbmt-key-ffi` from the tables its encoder\n\
         uses (`cargo test -p openbmt-key-ffi protocol`); the test fails when\n\
         this file differs. Do not edit it by hand.\n\
         \n\
         One function, `dispatch`, takes a request and returns a response.\n\
         \n\
         ```text\n\
         request   \"OBM8\" | version 1 | op u16 | handle u64 | fields...\n\
         response  \"OBM9\" | version 1 | status u8 | ...\n\
         \x20 status 0  ok: the result of the op\n\
         \x20 status 1  refused   then rule, text and words\n\
         \x20 status 2  store     then rule, text and words\n\
         \x20 status 3  state     then rule, text and words\n\
         \x20 status 4  internal  then rule, text and words\n\
         \x20 rule and text are each a u16 length and UTF-8\n\
         \x20 words: tone u8 | who u8 | heading | what | next\n\
         \x20   | code (flag, string) | at (flag, u64)\n\
         \x20   | quoted (flag, from, text) | detail (flag, string)\n\
         \x20   every string is a u16 length and UTF-8\n\
         ```\n\
         \n\
         Integers are little-endian. A bool is one byte, 0 or 1. A string is a\n\
         u32 length and UTF-8. `list<T>` is a u32 count and the elements;\n\
         `list<u8>` is the bytes type. `option<T>` is a flag byte 0 or 1 and\n\
         then the value. An enum is a tag byte and then the fields of its\n\
         variant. A record is its fields in the order given. A request is at\n\
         most 1048576 bytes, is read completely before anything is done, and\n\
         is refused when bytes are left over.\n\
         \n\
         ## Ops\n\
         \n\
         The handle names the object the op works on: `zero` (the field is 0),\n\
         `app` (from op 1) or `session` (from op 30).\n\
         \n\
         ```text\n",
    );
    for o in ops() {
        s.push_str(&format!(
            "{:>3} {:<22} handle {:<8} args {} -> {}\n",
            o.num,
            o.name,
            o.handle.name(),
            o.args,
            o.result
        ));
    }
    s.push_str("```\n\n## Types\n\n```text\n");
    for d in definitions() {
        s.push_str(&format!("{}: {}\n", d.name, d.body));
        if !d.tags.is_empty() {
            let tags: Vec<String> = d.tags.iter().map(|(v, t)| format!("{v}={t}")).collect();
            s.push_str(&format!("    tags {}\n", tags.join(" ")));
        }
    }
    s.push_str("```\n");
    let numbers = |names: Vec<String>| names.join(" ");
    let words = format!(
        "## Words\n\
         \n\
         The words of an error say what a person is shown. The numbers of the\n\
         tone and of the person who does the next step are:\n\
         \n\
         ```text\n\
         tone: {}\n\
         who:  {}\n\
         ```\n\
         \n\
         The time `at` is in seconds since 1970, for the app to show on the\n\
         person's own clock. `quoted` is text from another device, with where it\n\
         came from. `detail` is for the small print.\n\
         \n",
        numbers(TONES.iter().map(|(_, n, b)| format!("{n}={b}")).collect()),
        numbers(WHOS.iter().map(|(_, n, b)| format!("{n}={b}")).collect()),
    );
    s.replacen("## Ops\n", &format!("{words}## Ops\n"), 1)
}
