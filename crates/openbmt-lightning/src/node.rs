//! The calls the counter makes to the shop's node, whichever kind it is
//! (LN-07, LN-10, LN-13 to LN-15). Each call is a request for the app to send
//! and a reader of the reply.

use bitcoin::Network;

use crate::bolt11::{self, Checked, Expectation};
use crate::credential::Credential;
use crate::error::{InvoiceRefused, ReplyRefused};
use crate::invoice::{Ask, CreateOutcome, Created, Payment};
use crate::request::{Reply, Request};
use crate::{cln, lnd};

impl Credential {
    /// The request for the invoice of a sale or a test (LN-07, LN-10).
    pub fn create_request(&self, ask: &Ask) -> Request {
        match self {
            Self::Lnd(c) => lnd::create_request(c, ask),
            Self::Cln(c) => cln::create_request(c, ask),
        }
    }

    /// Read the reply to [`Credential::create_request`]. A refusal means no
    /// invoice was made that the counter can use. After a timeout, with no
    /// reply at all, the app sends the same request again: LND makes a new
    /// invoice, which is harmless because only the one shown is waited for;
    /// Core Lightning answers [`CreateOutcome::LabelExists`] if the first
    /// try got through (LN-15).
    pub fn parse_create(&self, reply: &Reply) -> Result<CreateOutcome, ReplyRefused> {
        match self {
            Self::Lnd(_) => lnd::parse_create(reply).map(CreateOutcome::Created),
            Self::Cln(_) => cln::parse_create(reply),
        }
    }

    /// LN-15: the request for the invoice that an earlier try made, after
    /// [`CreateOutcome::LabelExists`]. LND has no labels, so it never needs
    /// one and this is `None`.
    pub fn label_request(&self, ask: &Ask) -> Option<Request> {
        match self {
            Self::Lnd(_) => None,
            Self::Cln(c) => Some(cln::label_request(c, ask)),
        }
    }

    /// LN-15: read the reply to [`Credential::label_request`]. The invoice is
    /// taken up again only if its label, amount, description and state are
    /// this sale's.
    pub fn parse_existing(&self, reply: &Reply, ask: &Ask) -> Result<Created, ReplyRefused> {
        match self {
            Self::Lnd(_) => Err(ReplyRefused::LabelGone),
            Self::Cln(_) => cln::parse_existing(reply, ask),
        }
    }

    /// The request that asks whether the invoice with this payment hash is
    /// paid (LN-07, LN-10).
    pub fn lookup_request(&self, payment_hash: &[u8; 32]) -> Request {
        match self {
            Self::Lnd(c) => lnd::lookup_request(c, payment_hash),
            Self::Cln(c) => cln::lookup_request(c, payment_hash),
        }
    }

    /// LN-14: read the reply to [`Credential::lookup_request`]: paid only
    /// when the node says settled (LND) or paid (Core Lightning) and the
    /// amount received is at least `asked_msat`. An error is not a payment.
    pub fn parse_lookup(
        &self,
        reply: &Reply,
        payment_hash: &[u8; 32],
        asked_msat: u64,
    ) -> Result<Payment, ReplyRefused> {
        match self {
            Self::Lnd(_) => lnd::parse_lookup(reply, payment_hash, asked_msat),
            Self::Cln(_) => cln::parse_lookup(reply, payment_hash, asked_msat),
        }
    }
}

/// LN-17: check an invoice the node made against the sale, the shop's network
/// and the node key of the signed settings (`Settings::lightning_node`). The
/// invoice is shown only if this passes.
pub fn check_created(
    created: &Created,
    ask: &Ask,
    network: Network,
    node: [u8; 33],
) -> Result<Checked, InvoiceRefused> {
    bolt11::check(
        &created.bolt11,
        &Expectation {
            network,
            node,
            payment_hash: created.payment_hash,
            ask: ask.clone(),
        },
    )
}
