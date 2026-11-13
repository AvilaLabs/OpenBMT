//! The ops of the owner phone's web orders (`docs/WEB-ORDER-RULES.md`,
//! WO-05 to WO-32, ops 227 to 233): read a pairing code, pair a store, the
//! store's state, unpair, and the visits.
//!
//! This crate holds no rule of its own. The node keeps the pairing, the
//! addresses and the orders and decides every visit's next step; the app
//! sends what the node says to send, over HTTPS to the store's address and
//! through Tor when the shop has Tor on, and hands back the answer (op 232).
//! Every text is a placeholder that names its rule until the words are
//! written (`openbmt_owner::web_order::say`).

use openbmt_owner::starter::{KernelRandom, KERNEL_BYTES};
use openbmt_owner::web_order::{PairingCode, RangeResult};
use openbmt_owner_node::{
    trouble_words, OwnerNode, OwnerPort, PairState, WebCall, WebCallKind, WebError, WebFailure,
    WebPairShown, WebStateInfo, WebStep,
};

use crate::codec::{wire_record, wire_record_get, wire_unit_enum};
use crate::error::{from_words, rule_in, AppError, Kind};
use crate::requests::RequestAsk;
use crate::views::WordsView;

/// The kernel's bytes, from the `getrandom` system call (WO-43).
pub(crate) struct Kernel;

impl KernelRandom for Kernel {
    fn read32(&mut self) -> Option<[u8; KERNEL_BYTES]> {
        openbmt_kernel_random::read32().ok()
    }
}

/// The arguments of `web_pair_start` (WO-05, WO-06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebPairArgs {
    /// The time, the offset and the rate.
    pub ask: RequestAsk,
    /// The store's web address, as the owner typed it.
    pub url: String,
    /// The shop's name, which the code carries to the store.
    pub shop_name: String,
}

wire_record!(WebPairArgs {
    ask,
    url,
    shop_name
});
wire_record_get!(WebPairArgs {
    ask,
    url,
    shop_name
});

/// What `web_pair_start` gives (WO-05): the code to paste into the store's
/// settings and the fingerprint to compare with the one the store shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebPairView {
    /// The pairing code, in text.
    pub code: String,
    /// The key's short fingerprint.
    pub fingerprint: String,
    /// The shop's name in the code.
    pub shop_name: String,
    /// The store's address as it is kept.
    pub url: String,
}

wire_record!(WebPairView {
    code,
    fingerprint,
    shop_name,
    url
});

impl From<WebPairShown> for WebPairView {
    fn from(s: WebPairShown) -> Self {
        Self {
            code: s.code,
            fingerprint: s.fingerprint,
            shop_name: s.shop_name,
            url: s.url,
        }
    }
}

/// What `web_code_read` gives (WO-05): what a pairing code holds that the
/// owner compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebCodeView {
    /// The key's short fingerprint.
    pub fingerprint: String,
    /// The shop's name in the code.
    pub shop_name: String,
}

wire_record!(WebCodeView {
    fingerprint,
    shop_name
});

/// How far the pairing has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebPairingKind {
    /// No store is paired.
    None,
    /// The code is shown and waits for the owner's word that the
    /// fingerprints match.
    Waiting,
    /// The store is paired and visited.
    Paired,
}

wire_unit_enum!(WebPairingKind {
    None = 0,
    Waiting = 1,
    Paired = 2
});

/// One order the owner phone refused (WO-22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebRefusedView {
    /// The order number the store gave.
    pub order: u64,
    /// The address the store gave the customer.
    pub address: String,
    /// The satoshis the store asked for.
    pub sats: u64,
    /// When it was refused.
    pub at: u64,
    /// What is told of it.
    pub words: WordsView,
}

wire_record!(WebRefusedView {
    order,
    address,
    sats,
    at,
    words
});

/// What the store's page shows (WO-21, WO-22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebStateView {
    /// Why web orders are off, if they are.
    pub off: Option<WordsView>,
    /// How far the pairing has come.
    pub pairing: WebPairingKind,
    /// The shop's name in the pairing code.
    pub shop_name: String,
    /// The store's address.
    pub url: String,
    /// The fingerprint of the key.
    pub fingerprint: String,
    /// The pairing code, while it waits for the owner.
    pub code: Option<String>,
    /// When the store was last visited.
    pub last_visit: Option<u64>,
    /// What is told of the visits that fail.
    pub failure: Option<WordsView>,
    /// When the next visit may be, after failures.
    pub retry_at: Option<u64>,
    /// The addresses the store said it had left.
    pub left: Option<u64>,
    /// The addresses given to the store.
    pub addresses_sent: u32,
    /// What is told of a range the store did not take.
    pub trouble: Option<WordsView>,
    /// The orders taken.
    pub orders: u32,
    /// The orders waiting for a payment.
    pub waiting: u32,
    /// The orders the store was told were paid or part paid.
    pub told: u32,
    /// The notices not yet taken by the store.
    pub notices_waiting: u32,
    /// The orders refused, newest last.
    pub refused: Vec<WebRefusedView>,
}

wire_record!(WebStateView {
    off,
    pairing,
    shop_name,
    url,
    fingerprint,
    code,
    last_visit,
    failure,
    retry_at,
    left,
    addresses_sent,
    trouble,
    orders,
    waiting,
    told,
    notices_waiting,
    refused
});

/// The arguments of `web_step` (WO-21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebStepArgs {
    /// The time, the offset and the rate.
    pub ask: RequestAsk,
    /// A visit now, without the waiting after a failure: the owner asked, or
    /// the app was opened.
    pub force: bool,
}

wire_record!(WebStepArgs { ask, force });
wire_record_get!(WebStepArgs { ask, force });

/// What the node wants next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebStepKind {
    /// Nothing.
    Idle,
    /// A call is in the air; wait for its answer.
    Busy,
    /// Send the call.
    Call,
}

wire_unit_enum!(WebStepKind {
    Idle = 0,
    Busy = 1,
    Call = 2
});

/// Which call of a visit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebCallKindView {
    /// The orders made since the last visit (WO-20).
    Orders,
    /// A signed range of addresses (WO-10).
    Range,
    /// A signed notice (WO-30).
    Notice,
}

wire_unit_enum!(WebCallKindView {
    Orders = 0,
    Range = 1,
    Notice = 2
});

/// A header of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebHeaderView {
    /// The name.
    pub name: String,
    /// The value.
    pub value: String,
}

wire_record!(WebHeaderView { name, value });

/// One POST to the store, to be sent as it is (WO-20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebCallView {
    /// Which call it is.
    pub kind: WebCallKindView,
    /// The whole address.
    pub url: String,
    /// The headers of the HMAC.
    pub headers: Vec<WebHeaderView>,
    /// The value of the `Content-Type` header.
    pub content_type: String,
    /// The body, exactly as it must be sent.
    pub body: Vec<u8>,
}

wire_record!(WebCallView {
    kind,
    url,
    headers,
    content_type,
    body
});

/// What `web_step` gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebStepView {
    /// What the node wants.
    pub step: WebStepKind,
    /// The call to send, for [`WebStepKind::Call`].
    pub call: Option<WebCallView>,
}

wire_record!(WebStepView { step, call });

/// How a call ended, as the app saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebOutcome {
    /// The store answered: the status and the body are its.
    Answered,
    /// The app could not reach the store.
    NotReached,
    /// The shop has Tor on and Tor is not there.
    TorMissing,
    /// The answer was longer than the app reads.
    TooLong,
}

wire_unit_enum!(WebOutcome {
    Answered = 0,
    NotReached = 1,
    TorMissing = 2,
    TooLong = 3
});

/// The arguments of `web_reply` (WO-20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebReplyArgs {
    /// The time, the offset and the rate.
    pub ask: RequestAsk,
    /// How the call ended.
    pub outcome: WebOutcome,
    /// The HTTP status of the answer; 0 when there was none.
    pub status: u32,
    /// The body of the answer.
    pub body: Vec<u8>,
}

wire_record!(WebReplyArgs {
    ask,
    outcome,
    status,
    body
});
wire_record_get!(WebReplyArgs {
    ask,
    outcome,
    status,
    body
});

/// WO-05 and others: a refusal of the node's, with its rule and its words.
pub(crate) fn web_error(e: WebError) -> AppError {
    let kind = match &e {
        WebError::File(_) => Kind::Store,
        WebError::Off(_) | WebError::NotPaired | WebError::AlreadyPaired | WebError::Sequence => {
            Kind::State
        }
        _ => Kind::Refused,
    };
    let text = e.to_string();
    from_words(kind, rule_in(&text, "WO-05"), &text, e.words())
}

/// WO-05: what a pairing code holds, to compare.
pub(crate) fn read_code(text: String) -> Result<WebCodeView, AppError> {
    let code = PairingCode::from_text(&text).map_err(|r| web_error(WebError::Library(r)))?;
    Ok(WebCodeView {
        fingerprint: code.fingerprint(),
        shop_name: code.shop_name().to_string(),
    })
}

/// WO-05, WO-06: make the key and the code for a store.
pub(crate) fn pair_start<P: OwnerPort>(
    node: &mut OwnerNode<P>,
    a: &WebPairArgs,
) -> Result<WebPairView, AppError> {
    node.web_pair_start(&a.url, &a.shop_name, a.ask.now, &mut Kernel)
        .map(WebPairView::from)
        .map_err(web_error)
}

/// WO-21, WO-22: what the store's page shows.
pub(crate) fn state<P: OwnerPort>(node: &OwnerNode<P>) -> WebStateView {
    view_of(node.web_state())
}

fn view_of(info: WebStateInfo) -> WebStateView {
    WebStateView {
        off: info.off.map(|e| WordsView(e.words())),
        pairing: match info.pairing {
            None => WebPairingKind::None,
            Some(PairState::Waiting) => WebPairingKind::Waiting,
            Some(PairState::Paired) => WebPairingKind::Paired,
        },
        shop_name: info.shop_name,
        url: info.url,
        fingerprint: info.fingerprint,
        code: info.code,
        last_visit: info.last_visit,
        failure: info.failure.map(|f| WordsView(f.words())),
        retry_at: info.retry_at,
        left: info.left,
        addresses_sent: info.addresses_sent,
        trouble: info
            .trouble
            .map(|t: RangeResult| WordsView(trouble_words(t))),
        orders: info.orders,
        waiting: info.waiting,
        told: info.told,
        notices_waiting: info.notices_waiting,
        refused: info
            .refused
            .into_iter()
            .map(|r| WebRefusedView {
                order: r.order,
                address: r.address,
                sats: r.sats,
                at: r.at,
                words: WordsView(r.why.words()),
            })
            .collect(),
    }
}

fn call_view(call: WebCall) -> WebCallView {
    WebCallView {
        kind: match call.kind {
            WebCallKind::Orders => WebCallKindView::Orders,
            WebCallKind::Range => WebCallKindView::Range,
            WebCallKind::Notice => WebCallKindView::Notice,
        },
        url: call.url,
        headers: call
            .headers
            .into_iter()
            .map(|(name, value)| WebHeaderView { name, value })
            .collect(),
        content_type: call.content_type.to_string(),
        body: call.body,
    }
}

/// The view of a step that is not a request for indexes.
pub(crate) fn step_view(step: WebStep) -> Option<WebStepView> {
    match step {
        WebStep::Idle => Some(WebStepView {
            step: WebStepKind::Idle,
            call: None,
        }),
        WebStep::Busy => Some(WebStepView {
            step: WebStepKind::Busy,
            call: None,
        }),
        WebStep::Call(call) => Some(WebStepView {
            step: WebStepKind::Call,
            call: Some(call_view(call)),
        }),
        WebStep::NeedIndexes(_) => None,
    }
}

/// WO-20: give the node the answer to the call in the air, or tell it there
/// was none.
pub(crate) fn reply<P: OwnerPort>(
    node: &mut OwnerNode<P>,
    a: &WebReplyArgs,
) -> Result<WebStateView, AppError> {
    match a.outcome {
        WebOutcome::Answered => {
            let status = u16::try_from(a.status).map_err(|_| web_error(WebError::Sequence))?;
            node.web_reply(status, &a.body, a.ask.now)
                .map_err(web_error)?;
        }
        WebOutcome::NotReached => node.web_failed(WebFailure::NotReached, a.ask.now),
        WebOutcome::TorMissing => node.web_failed(WebFailure::TorMissing, a.ask.now),
        WebOutcome::TooLong => node.web_failed(WebFailure::TooLong, a.ask.now),
    }
    Ok(state(node))
}
