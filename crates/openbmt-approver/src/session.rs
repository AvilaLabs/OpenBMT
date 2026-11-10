//! An opened session: the lines, the decisions and the finish (AS-21 to
//! AS-37).

use std::cell::OnceCell;
use std::collections::BTreeMap;

use openbmt_session::{reason_text_ok, DeclineCode, ItemResult, Request, Response, Verdict};

use crate::error::{DecideError, FinishError};
use crate::evaluate::eval_item;
use crate::line::{Choice, Line};
use crate::state::{Approver, Kept};
use crate::MAX_SESSION_LOG;

/// An opened session. It borrows the Key app until it is dropped or
/// finished, so only one session is open at a time.
pub struct Session<'a> {
    pub(crate) approver: &'a mut Approver,
    pub(crate) request: Request,
    pub(crate) now: u64,
    choices: BTreeMap<u32, Choice>,
    /// The four session words of each item, worked out when first asked
    /// for: they take a moment.
    pub(crate) words: Vec<OnceCell<[&'static str; 4]>>,
    done: bool,
}

/// What evaluating the session gives: the lines, the results and the state
/// as it would be after the finish.
struct Evaluated {
    lines: Vec<Line>,
    results: Vec<ItemResult>,
    kept: Kept,
}

/// A decline's text made to fit the text rule of AS-12.
fn clip(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if out.len() + c.len_utf8() > openbmt_session::MAX_REASON_BYTES {
            break;
        }
        out.push(c);
    }
    if out.is_empty() {
        out.push('-');
    }
    out
}

impl<'a> Session<'a> {
    pub(crate) fn new(approver: &'a mut Approver, request: Request, now: u64) -> Self {
        let words = request.items().iter().map(|_| OnceCell::new()).collect();
        Self {
            approver,
            request,
            now,
            choices: BTreeMap::new(),
            words,
            done: false,
        }
    }

    /// The session's ID.
    pub fn session_id(&self) -> &[u8; 32] {
        self.request.session_id()
    }

    /// The request that was opened.
    pub fn request(&self) -> &Request {
        &self.request
    }

    /// AS-31, AS-32: take the lines in item order. A line sees what the
    /// approved lines before it did.
    fn evaluate(&self) -> Evaluated {
        let mut kept = self.approver.kept.clone();
        let mut lines = Vec::with_capacity(self.request.items().len());
        let mut results = Vec::with_capacity(self.request.items().len());
        for (index, entry) in self.request.items().iter().enumerate() {
            let choice = self.choices.get(&entry.number);
            let apply = matches!(choice, Some(Choice::Approve));
            let checked = eval_item(self, index, entry, &mut kept, apply);
            let verdict = match (&checked.block, choice, checked.payload) {
                // AS-36: without key 2 a spend is answered "not on this
                // device", whatever was decided.
                (Some(b), _, _) if b.code == DeclineCode::NotOnThisDevice => Verdict::Declined {
                    code: b.code,
                    text: clip(&b.text),
                },
                (None, Some(Choice::Approve), Some(payload)) => Verdict::Approved(payload),
                (Some(b), Some(Choice::Approve), _) => Verdict::Declined {
                    code: b.code,
                    text: clip(&b.text),
                },
                (_, Some(Choice::Decline(reason)), _) => Verdict::Declined {
                    code: DeclineCode::ByPerson,
                    text: clip(reason),
                },
                (_, None, _) => Verdict::Declined {
                    code: DeclineCode::NotDecided,
                    text: "not decided".to_string(),
                },
                (None, Some(Choice::Approve), None) => Verdict::Declined {
                    code: DeclineCode::ChecksFailed,
                    text: "AS-33: nothing was signed for this line".to_string(),
                },
            };
            results.push(ItemResult {
                number: entry.number,
                verdict,
            });
            lines.push(Line {
                item: entry.number,
                kind: entry.item.kind(),
                detail: checked.detail,
                can_approve: checked.block.is_none(),
                failure: checked.block.map(|b| b.text),
                decision: choice.cloned(),
            });
        }
        Evaluated {
            lines,
            results,
            kept,
        }
    }

    /// AS-23 to AS-29: one line for each item, from the Key app's own
    /// checks, with the decisions made so far. It signs and stores nothing
    /// (AS-21). After the finish it returns no lines.
    pub fn lines(&self) -> Vec<Line> {
        if self.done {
            return Vec::new();
        }
        self.evaluate().lines
    }

    /// AS-30: decide one line. A line that cannot be approved cannot be
    /// approved here. A decision can be changed until the finish.
    pub fn decide(&mut self, item: u32, choice: Choice) -> Result<(), DecideError> {
        if self.done {
            return Err(DecideError::Finished);
        }
        if self.request.item(item).is_none() {
            return Err(DecideError::UnknownItem(item));
        }
        match &choice {
            Choice::Decline(reason) => {
                if !reason_text_ok(reason) {
                    return Err(DecideError::BadReason);
                }
            }
            Choice::Approve => {
                // The line as it stands with the decisions before it.
                let previous = self.choices.remove(&item);
                let lines = self.lines();
                if let Some(p) = previous {
                    self.choices.insert(item, p);
                }
                let line = lines.iter().find(|l| l.item == item);
                if let Some(Line {
                    can_approve: false,
                    failure,
                    ..
                }) = line
                {
                    return Err(DecideError::CannotApprove(
                        failure.clone().unwrap_or_default(),
                    ));
                }
            }
        }
        self.choices.insert(item, choice);
        Ok(())
    }

    /// AS-33 to AS-37: sign every approved line through the existing
    /// crates, hand all new state to `persist` as one snapshot, and only
    /// then return the response bytes. If `persist` fails nothing is
    /// returned, nothing the Key app keeps has changed, and the session can
    /// be finished again.
    pub fn finish<E>(
        &mut self,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Vec<u8>, FinishError<E>> {
        if self.done {
            return Err(FinishError::Finished);
        }
        let Evaluated {
            results, mut kept, ..
        } = self.evaluate();
        // AS-37: the response is built and its size checked before anything
        // is stored.
        let response = Response::new(
            &self.approver.identity,
            *self.request.session_id(),
            self.request.hash(),
            results,
        )
        .map_err(FinishError::Response)?;
        // AS-22: the ID joins the log in the same store.
        kept.seen.push(*self.request.session_id());
        if kept.seen.len() > MAX_SESSION_LOG {
            kept.seen.remove(0);
        }
        // AS-34: stored before released.
        persist(&self.approver.snapshot_of(&kept)).map_err(FinishError::Store)?;
        self.approver.kept = kept;
        self.done = true;
        Ok(response.as_bytes().to_vec())
    }
}
