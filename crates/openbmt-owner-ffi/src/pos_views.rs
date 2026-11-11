//! The types of the till ops (`docs/OWNER-COINS-RULES.md`, OC-30 to OC-37):
//! what the owner phone sends and reads to match the shop's till against
//! the counters' sales. They are written and read like those of
//! [`views`](crate::views).

use crate::codec::{
    bad_tag, wire_record, wire_record_get, wire_unit_enum, Define, Get, Put, Reader, Shaped,
    TypeDef,
};
use crate::error::AppError;
use crate::views::WordsView;

/// Which vendor's till the owner reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosVendorView {
    /// Square.
    Square,
    /// Clover.
    Clover,
}

wire_unit_enum!(PosVendorView {
    Square = 0,
    Clover = 1
});

/// The owner's settings for the till: what `pos_setup` takes and `pos_state`
/// shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosSetupView {
    /// The vendor.
    pub vendor: PosVendorView,
    /// Square's location id, or Clover's merchant id.
    pub location: String,
    /// The exact names of the tender that means Bitcoin.
    pub tender_names: Vec<String>,
    /// The shop's currencies, three capitals each.
    pub currencies: Vec<String>,
}

wire_record!(PosSetupView {
    vendor,
    location,
    tender_names,
    currencies
});
wire_record_get!(PosSetupView {
    vendor,
    location,
    tender_names,
    currencies
});

/// How the app sends the request of `pos_request`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosMethodView {
    /// A GET with no body.
    Get,
    /// A POST of a JSON body.
    Post,
}

wire_unit_enum!(PosMethodView {
    Get = 0,
    Post = 1
});

/// What `pos_request` returns: the request to send, without the sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosRequestView {
    /// The method.
    pub method: PosMethodView,
    /// The whole address, with its query.
    pub url: String,
    /// The body; empty for a GET.
    pub body: Vec<u8>,
}

wire_record!(PosRequestView { method, url, body });

/// The arguments of `pos_reply`: the till's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosReplyArgs {
    /// The time now, in seconds since 1970.
    pub now: u64,
    /// The status the till answered with.
    pub status: u32,
    /// The body, at most 8 MiB.
    pub body: Vec<u8>,
}

wire_record!(PosReplyArgs { now, status, body });
wire_record_get!(PosReplyArgs { now, status, body });

/// Which alert of the matcher (`docs/POS-RULES.md`, PS-40 to PS-48).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosAlertKind {
    /// A Bitcoin tender with no sale.
    TenderWithoutPayment,
    /// The order was closed before the payment was safe.
    ClosedBeforePaid,
    /// A paid sale with no tender.
    PaymentWithoutTender,
    /// A reference named a sale whose amount differs.
    AmountDiffers,
    /// The matched sale is not paid.
    TenderOnUnpaidSale,
    /// The order or tender was voided or refunded after payment.
    VoidedAfterPayment,
    /// One sale named by two tenders.
    DuplicateReference,
    /// A tip on a Bitcoin tender.
    BitcoinTip,
    /// A currency the shop does not use.
    UnknownCurrency,
}

wire_unit_enum!(PosAlertKind {
    TenderWithoutPayment = 0,
    ClosedBeforePaid = 1,
    PaymentWithoutTender = 2,
    AmountDiffers = 3,
    TenderOnUnpaidSale = 4,
    VoidedAfterPayment = 5,
    DuplicateReference = 6,
    BitcoinTip = 7,
    UnknownCurrency = 8
});

/// What a till alert is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosSubjectView {
    /// A tender of an order, as the vendor names them.
    Tender {
        /// The order.
        order: String,
        /// The tender.
        tender: String,
    },
    /// A counter sale.
    Sale {
        /// The counter, 32 bytes.
        counter: Vec<u8>,
        /// The sale's receive index.
        index: u32,
    },
}

impl Shaped for PosSubjectView {
    fn shape() -> String {
        "PosSubjectView".to_string()
    }
}

impl Put for PosSubjectView {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            Self::Tender { order, tender } => {
                out.push(0);
                order.put(out);
                tender.put(out);
            }
            Self::Sale { counter, index } => {
                out.push(1);
                counter.put(out);
                index.put(out);
            }
        }
    }
}

impl Get for PosSubjectView {
    fn get(r: &mut Reader<'_>) -> Result<Self, AppError> {
        match r.u8()? {
            0 => Ok(Self::Tender {
                order: String::get(r)?,
                tender: String::get(r)?,
            }),
            1 => Ok(Self::Sale {
                counter: Vec::<u8>::get(r)?,
                index: u32::get(r)?,
            }),
            t => Err(bad_tag("PosSubjectView", t)),
        }
    }
}

impl Define for PosSubjectView {
    fn define() -> TypeDef {
        TypeDef {
            name: "PosSubjectView",
            body: "enum, a tag byte, then: Tender(order: string, tender: string) | \
                   Sale(counter: list<u8>, index: u32)"
                .to_string(),
            tags: vec![("Tender", 0), ("Sale", 1)],
        }
    }
}

/// An alert's identity: what `pos_acknowledge` is given, as `pos_state`
/// showed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosAlertKey {
    /// Which alert.
    pub kind: PosAlertKind,
    /// What it is about.
    pub subject: PosSubjectView,
}

wire_record!(PosAlertKey { kind, subject });
wire_record_get!(PosAlertKey { kind, subject });

/// The arguments of `pos_acknowledge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosAckArgs {
    /// The time now, in seconds since 1970.
    pub now: u64,
    /// The alert.
    pub key: PosAlertKey,
}

wire_record!(PosAckArgs { now, key });
wire_record_get!(PosAckArgs { now, key });

/// An alert of the till, open or new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosAlertView {
    /// Its identity.
    pub key: PosAlertKey,
    /// When it was raised, in seconds since 1970.
    pub raised_at: u64,
    /// Why it was raised and what to do next.
    pub words: WordsView,
}

wire_record!(PosAlertView {
    key,
    raised_at,
    words
});

/// What `pos_reply` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosReplyView {
    /// Another page is wanted now: ask for the next request.
    pub again: bool,
    /// The till has more than this read took; the next read goes on.
    pub more_left: bool,
    /// Pages taken in this read so far.
    pub pages: u32,
    /// Orders taken in this read so far.
    pub orders_read: u32,
    /// Matches that are new.
    pub new_matches: u32,
    /// Alerts that are new.
    pub new_alerts: Vec<PosAlertView>,
    /// Alerts that cleared because their condition went.
    pub cleared: u32,
    /// Matches retired into the count.
    pub retired: u64,
}

wire_record!(PosReplyView {
    again,
    more_left,
    pages,
    orders_read,
    new_matches,
    new_alerts,
    cleared,
    retired
});

/// What `pos_state` returns: what the till screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosStateView {
    /// The connection, if there is one.
    pub setup: Option<PosSetupView>,
    /// When a read last finished in this run of the app.
    pub last_poll_at: Option<u64>,
    /// The open alerts, oldest first.
    pub open_alerts: Vec<PosAlertView>,
    /// Alerts the owner acknowledged.
    pub acknowledged_alerts: u32,
    /// Orders the matcher tracks.
    pub tracked_orders: u32,
    /// Matches held.
    pub matches: u32,
    /// Of those, matched by order of time and not exact.
    pub matches_by_order: u32,
    /// Matches retired and counted.
    pub retired_matches: u64,
}

wire_record!(PosStateView {
    setup,
    last_poll_at,
    open_alerts,
    acknowledged_alerts,
    tracked_orders,
    matches,
    matches_by_order,
    retired_matches
});
