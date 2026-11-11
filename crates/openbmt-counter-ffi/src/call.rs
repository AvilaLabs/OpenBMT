//! The call protocol: one function, [`dispatch`], that takes a framed
//! request and returns a framed response. Every call the counter app makes
//! on the node is one op of it, so a platform binding needs one entry point
//! and no generated code.
//!
//! ```text
//! request   "OBMm" | version 1 | op u16 | handle u64 | fields...
//! response  "OBMn" | version 1 | status u8 | ...
//!   status 0  ok: the result of the op
//!   status 1  refused  (CounterError::Refused)
//!   status 2  store    (CounterError::Store)
//!   status 3  state    (CounterError::State)
//!   status 4  internal (CounterError::Internal, a panic was caught)
//!   status 1 to 4 carry rule and text, then the words (see below)
//! ```
//!
//! After rule and text an error response carries the words the person is
//! shown (`docs/SCREEN-RULES.md`): tone u8, who u8, heading, what, next,
//! code (a flag byte, then a string), at (a flag byte, then a u64), quoted
//! (a flag byte, then from and text) and detail (a flag byte, then a
//! string), every string a u16 length and UTF-8. A result that shows a
//! state to a person carries the same words in the same form.
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
//! - 1 `start`, 2 `stop`: a node and its handle.
//! - 10 `pump`; 11 to 15 the sale (`open`, `judge`, `requote`, `cancel`,
//!   `override_sale`); 16 to 19 refunds; 20 to 23 what the owner sends
//!   (`heartbeat`, `command`, `check_in`, `add_batch`).
//! - 30 `status`, 31 `notes`.
//!
//! The handle names the node an op works on (from `start`), or must be 0 for
//! `start`. Handles start at 1 and are never reused in a process. A
//! [`Registry`] holds at most [`MAX_NODES`] nodes.
//!
//! Unlike the Key app's calls, these may run at the same time: the app
//! calls `pump` from one thread and the other ops from another. See
//! [`host`](crate::host) for how they share the node.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::codec::{Define, Get, Put, Reader, TypeDef, MAX_REQUEST};
use crate::error::{internal, refused_by, wrong_state, CounterError};
use crate::host::{real_spawn, Entry, Spawn};
use crate::pay::Payment;
use crate::say::{TONES, WHOS};
use crate::views::{
    put_words, ActorRole, ActorView, CashArgs, CounterNetwork, HealthView, HeartbeatOutcome,
    OpenArgs, OpenedView, OverrideArgs, PumpView, RefundArgs, RefundAskView, RefundRow,
    RefundStateView, ReportView, RequoteArgs, ScreenView, StartConfig, StateView, StatusView,
    WithdrawArgs, WordsView,
};

/// The first four bytes of a request.
pub const REQUEST_MAGIC: [u8; 4] = *b"OBMm";
/// The first four bytes of a response.
pub const RESPONSE_MAGIC: [u8; 4] = *b"OBMn";
/// The version of both frames.
pub const VERSION: u8 = 1;

/// The most nodes a registry holds.
pub const MAX_NODES: usize = 2;

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
    /// A node handle.
    Node,
}

impl Handle {
    fn name(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::Node => "node",
        }
    }
}

struct RegState {
    next: u64,
    nodes: BTreeMap<u64, Arc<Entry>>,
}

/// The running nodes of a process, by handle. [`dispatch`] takes one by
/// reference; a platform binding keeps one in a static.
pub struct Registry {
    state: Mutex<RegState>,
    /// The way the next node starts its driver, in tests.
    #[cfg(test)]
    pub(crate) spawner: Mutex<Option<Spawn>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// An empty registry. The first handle given out is 1.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(RegState {
                next: 1,
                nodes: BTreeMap::new(),
            }),
            #[cfg(test)]
            spawner: Mutex::new(None),
        }
    }

    fn lock(&self) -> MutexGuard<'_, RegState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn check_room(&self) -> Result<(), CounterError> {
        if self.lock().nodes.len() >= MAX_NODES {
            return Err(wrong_state(
                "node-limit",
                "the most nodes are running: stop one before starting another",
            ));
        }
        Ok(())
    }

    fn spawn(&self) -> Spawn {
        #[cfg(test)]
        {
            if let Some(spawn) = self
                .spawner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                return spawn;
            }
        }
        real_spawn()
    }

    fn start(&self, config: StartConfig) -> Result<u64, CounterError> {
        self.check_room()?;
        let entry = Arc::new(Entry::start(config, self.spawn())?);
        let mut state = self.lock();
        if state.nodes.len() >= MAX_NODES {
            drop(state);
            // Another start got the room first; let go of this node.
            let _ = entry.stop();
            return Err(wrong_state(
                "node-limit",
                "the most nodes are running: stop one before starting another",
            ));
        }
        let handle = state.next;
        match state.next.checked_add(1) {
            Some(next) => state.next = next,
            None => {
                drop(state);
                let _ = entry.stop();
                return Err(wrong_state(
                    "handles-used",
                    "every handle of this process is used",
                ));
            }
        }
        state.nodes.insert(handle, entry);
        Ok(handle)
    }

    /// The entry of a node, for a test.
    #[cfg(test)]
    pub(crate) fn entry_for_test(&self, handle: u64) -> Option<Arc<Entry>> {
        self.lock().nodes.get(&handle).cloned()
    }

    fn remove(&self, handle: u64) -> Result<Arc<Entry>, CounterError> {
        self.lock().nodes.remove(&handle).ok_or_else(unknown_handle)
    }

    fn resolve(&self, kind: Handle, handle: u64) -> Result<Ctx<'_>, CounterError> {
        let mut ctx = Ctx {
            registry: self,
            handle,
            entry: None,
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
            Handle::Node => {
                ctx.entry = Some(
                    self.lock()
                        .nodes
                        .get(&handle)
                        .cloned()
                        .ok_or_else(unknown_handle)?,
                );
            }
        }
        Ok(ctx)
    }
}

fn unknown_handle() -> CounterError {
    refused_by(
        "handle-unknown",
        "no node has this handle: it was never started or has been stopped",
    )
}

/// What an op works with.
pub(crate) struct Ctx<'a> {
    registry: &'a Registry,
    handle: u64,
    entry: Option<Arc<Entry>>,
}

impl Ctx<'_> {
    fn entry(&self) -> Result<&Arc<Entry>, CounterError> {
        self.entry
            .as_ref()
            .ok_or_else(|| internal("the op has no node"))
    }
}

type Run = Box<dyn Fn(&Ctx<'_>, &mut Reader<'_>) -> Result<Vec<u8>, CounterError> + Send + Sync>;

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
    f: fn(&Ctx<'_>, A) -> Result<R, CounterError>,
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
    use Handle::{Node, Zero};
    vec![
        op(1, "start", Zero, |c, config: StartConfig| {
            c.registry.start(config)
        }),
        op(2, "stop", Node, |c, ()| {
            let entry = c.registry.remove(c.handle)?;
            entry.stop()
        }),
        op(10, "pump", Node, |c, (now, wait_ms): (u64, u32)| {
            c.entry()?.pump(now, wait_ms)
        }),
        op(11, "open", Node, |c, a: OpenArgs| c.entry()?.open(a)),
        op(12, "judge", Node, |c, (now, index): (u64, u32)| {
            c.entry()?.judge(now, index)
        }),
        op(13, "requote", Node, |c, a: RequoteArgs| {
            c.entry()?.requote(a)
        }),
        op(14, "cancel", Node, |c, (now, index): (u64, u32)| {
            c.entry()?.cancel(now, index)
        }),
        op(15, "override_sale", Node, |c, a: OverrideArgs| {
            c.entry()?.override_sale(a)
        }),
        op(16, "request_refund", Node, |c, a: RefundArgs| {
            c.entry()?.request_refund(a)
        }),
        op(17, "withdraw_refund", Node, |c, a: WithdrawArgs| {
            c.entry()?.withdraw_refund(a)
        }),
        op(18, "refund_in_cash", Node, |c, a: CashArgs| {
            c.entry()?.refund_in_cash(a)
        }),
        op(19, "refunds", Node, |c, index: u32| {
            c.entry()?.refunds(index)
        }),
        op(20, "heartbeat", Node, |c, (now, bytes): (u64, Vec<u8>)| {
            c.entry()?.heartbeat(now, &bytes)
        }),
        op(21, "command", Node, |c, (now, bytes): (u64, Vec<u8>)| {
            c.entry()?.command(now, &bytes)
        }),
        op(22, "check_in", Node, |c, now: u64| c.entry()?.check_in(now)),
        op(23, "add_batch", Node, |c, (now, bytes): (u64, Vec<u8>)| {
            c.entry()?.add_batch(now, &bytes)
        }),
        op(30, "status", Node, |c, ()| c.entry()?.status()),
        op(31, "notes", Node, |c, ()| c.entry()?.notes()),
    ]
}

/// The op with this number. Under test there is one more, which panics.
fn find_op(num: u16) -> Option<&'static Op> {
    #[cfg(test)]
    {
        static TEST_OPS: OnceLock<Vec<Op>> = OnceLock::new();
        let extra = TEST_OPS.get_or_init(|| {
            vec![op(
                65_000,
                "test_panic",
                Handle::Zero,
                |_, ()| -> Result<(), CounterError> { panic!("test panic with a secret in it") },
            )]
        });
        if let Some(o) = extra.iter().find(|o| o.num == num) {
            return Some(o);
        }
    }
    ops().iter().find(|o| o.num == num)
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

/// Make the call: the header, the op, the handle, the op.
fn call(registry: &Registry, request: &[u8]) -> Result<Vec<u8>, CounterError> {
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
            "the request does not start OBMm",
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
    let op = find_op(num)
        .ok_or_else(|| refused_by("op-unknown", format!("{num} is not the number of an op")))?;
    let ctx = registry.resolve(op.handle, handle)?;
    match catch_unwind(AssertUnwindSafe(|| (op.run)(&ctx, &mut r))) {
        Ok(result) => result,
        Err(p) => Err(internal(format!("caught a panic: {}", panic_text(&*p)))),
    }
}

/// Run one request. The response is always a frame: a result, or an error
/// with its rule, text and words. A panic anywhere in the call becomes
/// status 4.
///
/// A request that is not read completely, or names an op or a handle that
/// does not exist, changes nothing.
pub fn dispatch(registry: &Registry, request: &[u8]) -> Vec<u8> {
    match catch_unwind(AssertUnwindSafe(|| call(registry, request))) {
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

/// The response frame of an error. For a binding that fails before it can
/// call [`dispatch`].
pub fn error_response(e: &CounterError) -> Vec<u8> {
    let status = match e {
        CounterError::Refused { .. } => STATUS_REFUSED,
        CounterError::Store { .. } => STATUS_STORE,
        CounterError::State { .. } => STATUS_STATE,
        CounterError::Internal { .. } => STATUS_INTERNAL,
    };
    let mut out = Vec::new();
    out.extend_from_slice(&RESPONSE_MAGIC);
    out.push(VERSION);
    out.push(status);
    put_str16(&mut out, e.rule());
    // Panic payloads are arbitrary program data and may contain secrets.
    // Do not copy one into the response frame sent to the platform app.
    let text = if matches!(e, CounterError::Internal { text, .. } if text.starts_with("caught a panic: "))
    {
        "caught a panic: [redacted]"
    } else {
        e.text()
    };
    put_str16(&mut out, text);
    put_words(&mut out, e.words());
    out
}

// ---- the description of the protocol, generated from the code above ----------

fn definitions() -> Vec<TypeDef> {
    vec![
        CounterNetwork::define(),
        ActorRole::define(),
        ActorView::define(),
        StartConfig::define(),
        OpenArgs::define(),
        RequoteArgs::define(),
        OverrideArgs::define(),
        RefundAskView::define(),
        RefundArgs::define(),
        WithdrawArgs::define(),
        CashArgs::define(),
        WordsView::define(),
        StateView::define(),
        RefundStateView::define(),
        ReportView::define(),
        Payment::define(),
        OpenedView::define(),
        ScreenView::define(),
        PumpView::define(),
        RefundRow::define(),
        HeartbeatOutcome::define(),
        HealthView::define(),
        StatusView::define(),
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
        "# The counter call protocol\n\
         \n\
         Generated by a test of `openbmt-counter-ffi` from the tables its\n\
         encoder uses (`cargo test -p openbmt-counter-ffi protocol`); the test\n\
         fails when this file differs. Do not edit it by hand.\n\
         \n\
         One function, `dispatch`, takes a request and returns a response.\n\
         \n\
         ```text\n\
         request   \"OBMm\" | version 1 | op u16 | handle u64 | fields...\n\
         response  \"OBMn\" | version 1 | status u8 | ...\n\
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
         u32 length and UTF-8 (a `str16` is a u16 length and UTF-8, and is\n\
         used only inside `Words`). `list<T>` is a u32 count and the\n\
         elements; `list<u8>` is the bytes type. `option<T>` is a flag byte 0\n\
         or 1 and then the value. An enum is a tag byte and then the fields of\n\
         its variant. A record is its fields in the order given. A request is\n\
         at most 1048576 bytes, is read completely before anything is done,\n\
         and is refused when bytes are left over.\n\
         \n\
         Times are seconds since 1970. Money is in minor units of the shop's\n\
         currency or in satoshis, as the field says. The QR modules of a\n\
         `Payment` are `qr_size` squared bits, dark as 1, in one stream most\n\
         significant bit first, row by row, the last byte padded with 0.\n\
         \n\
         ## Ops\n\
         \n\
         The handle names the node the op works on: `zero` (the field is 0) or\n\
         `node` (from `start`). `pump` may run on one thread while the other\n\
         ops run on another; it waits for at most 250 ms.\n\
         \n\
         ```text\n",
    );
    for o in ops() {
        s.push_str(&format!(
            "{:>3} {:<16} handle {:<5} args {} -> {}\n",
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
         The words say what a person is shown, for an error and for every\n\
         state a result carries. The numbers of the tone and of the person who\n\
         does the next step are:\n\
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
