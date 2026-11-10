//! One approval session as an object the app holds.
//!
//! The approver's `Session` borrows the approver, so the handle keeps only
//! what is needed to open the session again: the request bytes, the time it
//! was opened at, and the decisions that were taken, in order. Every call
//! opens the request on the approver, takes the decisions again in that
//! order (each is checked by the approver as it was the first time) and
//! then does what was asked. The approver's own `Session` does the work;
//! nothing about a line is decided here.

use std::sync::{Arc, Mutex};

use openbmt_approver::{Approver, Choice, DecideError, FinishError, Session};

use crate::app::{save_ready, KeyApp, Phase, ReadyState};
use crate::error::{internal, refused, store_failed, wrong_state, KeyError};
use crate::views::{line_view, Decision, LineView};

struct HandleState {
    /// The decisions taken so far, in the order they were taken.
    log: Vec<(u32, Choice)>,
    /// The lines for the decisions in `log`, worked out when first asked
    /// for: the four session words of a pairing take a moment.
    cache: Option<Vec<LineView>>,
    done: bool,
}

/// An open approval session. Only one is open at a time. Dropping the
/// handle without `finish` changes nothing; call `cancel` (or close the
/// object) to release the slot at once instead of waiting for the garbage
/// collector.
pub struct SessionHandle {
    app: Arc<KeyApp>,
    id: u64,
    request: Vec<u8>,
    now: u64,
    state: Mutex<HandleState>,
}

/// The request opened on `approver` with `log` taken again.
fn reopen<'a>(
    approver: &'a mut Approver,
    request: &[u8],
    now: u64,
    log: &[(u32, Choice)],
) -> Result<Session<'a>, KeyError> {
    let mut session = approver.open(request, now).map_err(refused)?;
    for (item, choice) in log {
        session
            .decide(*item, choice.clone())
            .map_err(|e| internal(format!("a decision taken before was refused now: {e}")))?;
    }
    Ok(session)
}

impl KeyApp {
    /// AS-20: read and check a request and open the session. Nothing is
    /// stored or released. A second session is refused while one is open.
    pub fn open_session(
        self: &Arc<Self>,
        request: Vec<u8>,
    ) -> Result<Arc<SessionHandle>, KeyError> {
        let (id, now) = self.run(|inner| {
            if inner.session.is_some() {
                return Err(wrong_state(
                    "session-open",
                    "a session is open already: finish or cancel it first",
                ));
            }
            let now = inner.store.now();
            let id = inner.next_session;
            let ready = inner.ready()?;
            // The checks of AS-20 run here; the session itself is dropped.
            ready
                .approver
                .open(&request, now)
                .map(drop)
                .map_err(refused)?;
            inner.next_session = id.saturating_add(1);
            inner.session = Some(id);
            Ok((id, now))
        })?;
        Ok(Arc::new(SessionHandle {
            app: Arc::clone(self),
            id,
            request,
            now,
            state: Mutex::new(HandleState {
                log: Vec::new(),
                cache: None,
                done: false,
            }),
        }))
    }
}

impl SessionHandle {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HandleState>, KeyError> {
        self.state
            .lock()
            .map_err(|_| internal("an earlier panic poisoned this session"))
    }

    /// Run `f` on the approver with the lock held, if this handle's session
    /// is still the open one.
    fn with_approver<T>(
        &self,
        f: impl FnOnce(&mut ReadyState) -> Result<T, KeyError>,
    ) -> Result<T, KeyError> {
        self.app.run(|inner| {
            if inner.session != Some(self.id) {
                return Err(wrong_state(
                    "session-closed",
                    "this session is no longer open",
                ));
            }
            f(inner.ready()?)
        })
    }
}

impl SessionHandle {
    /// The session's ID, 32 bytes.
    pub fn session_id(&self) -> Result<Vec<u8>, KeyError> {
        let log = self.lock()?.log.clone();
        self.with_approver(|ready| {
            let session = reopen(&mut ready.approver, &self.request, self.now, &log)?;
            Ok(session.session_id().to_vec())
        })
    }

    /// AS-23 to AS-29: one line for each item, from the Key app's own
    /// checks, with the decisions made so far. After the finish: no lines.
    pub fn lines(&self) -> Result<Vec<LineView>, KeyError> {
        let mut state = self.lock()?;
        if state.done {
            return Ok(Vec::new());
        }
        if let Some(cached) = &state.cache {
            return Ok(cached.clone());
        }
        let log = state.log.clone();
        let lines = self.with_approver(|ready| {
            let session = reopen(&mut ready.approver, &self.request, self.now, &log)?;
            Ok(session.lines().iter().map(line_view).collect::<Vec<_>>())
        })?;
        state.cache = Some(lines.clone());
        Ok(lines)
    }

    /// AS-30: decide one line. A line that cannot be approved cannot be
    /// approved here, and a decision can be changed until the finish.
    pub fn decide(&self, item: u32, decision: Decision) -> Result<(), KeyError> {
        let mut state = self.lock()?;
        if state.done {
            return Err(refused(DecideError::Finished));
        }
        let log = state.log.clone();
        let choice: Choice = decision.into();
        self.with_approver(|ready| {
            let mut session = reopen(&mut ready.approver, &self.request, self.now, &log)?;
            session.decide(item, choice.clone()).map_err(refused)
        })?;
        state.log.push((item, choice));
        state.cache = None;
        Ok(())
    }

    /// AS-33 to AS-37: sign the approved lines through the existing
    /// crates, store all new state through `KeyStore.save`, and only then
    /// return the response bytes. If the store fails nothing is returned,
    /// nothing the Key app keeps has changed, and the session can be
    /// finished again.
    pub fn finish(&self) -> Result<Vec<u8>, KeyError> {
        let mut state = self.lock()?;
        if state.done {
            return Err(refused(FinishError::<String>::Finished));
        }
        let log = state.log.clone();
        let response = self.app.run(|inner| {
            if inner.session != Some(self.id) {
                return Err(wrong_state(
                    "session-closed",
                    "this session is no longer open",
                ));
            }
            let store = Arc::clone(&inner.store);
            let Phase::Ready(ready) = &mut inner.phase else {
                return Err(internal("the session has no wallet"));
            };
            let ReadyState {
                material,
                wallet,
                approver,
            } = &mut **ready;
            let mut session = reopen(approver, &self.request, self.now, &log)?;
            let response = session
                .finish(|snapshot| save_ready(&store, material, wallet, snapshot))
                .map_err(|e| match e {
                    FinishError::Store(why) => store_failed(&why),
                    other => refused(other),
                })?;
            inner.session = None;
            Ok(response)
        })?;
        state.done = true;
        state.cache = None;
        Ok(response)
    }

    /// Release the session slot without finishing. Nothing is stored and
    /// nothing changes. Safe to call more than once.
    pub fn cancel(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.done = true;
            state.cache = None;
        }
        release(&self.app, self.id);
    }
}

fn release(app: &KeyApp, id: u64) {
    if let Ok(mut inner) = app.inner.lock() {
        if inner.session == Some(id) {
            inner.session = None;
        }
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        release(&self.app, self.id);
    }
}
