//! The test of a connection at setup (LN-06, LN-09, LN-20).
//!
//! The node software enforces the limit of a credential, and the runes of
//! Core Lightning have had fixes in security releases, so reading a
//! credential is never enough: setup also tries to do what the credential
//! must not be able to do, and then makes one 1-satoshi invoice and checks
//! that it is signed by the node the settings name. The test is a chain of
//! three requests for the app to send, one after the other:
//!
//! 1. a read-only call that a credential limited to invoices must be refused;
//! 2. a second such call;
//! 3. the test invoice, which must pass `bolt11::check`.
//!
//! Nothing here calls anything that moves money.

use bitcoin::Network;

use crate::bolt11::Checked;
use crate::credential::Credential;
use crate::error::{ConnectionRefused, InvoiceRefused};
use crate::invoice::{Ask, CreateOutcome};
use crate::node::check_created;
use crate::request::{Reply, Request};
use crate::{cln, lnd};

/// What a good test leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected {
    /// The test invoice that passed every check, signed by the node of the
    /// settings. It is never paid; it runs out by itself.
    pub test_invoice: Checked,
}

/// Where the test is after a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Send the next request ([`ConnectionTest::request`]).
    More(ConnectionTest),
    /// The connection is good.
    Done(Connected),
}

/// What a probe showed (LN-06, LN-09).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Probe {
    /// The node refused the call with its permission error.
    Denied,
    /// The call worked: the credential can do more than invoices.
    Allowed,
    /// Core Lightning says the rune is not from this node (code 1501).
    NotFromNode,
    /// Core Lightning says the rune has been revoked (code 1503).
    Revoked,
    /// The call failed in another way, so the credential is not known to be
    /// limited.
    Unclear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    ProbeOne,
    ProbeTwo,
    Invoice,
}

/// A connection test in progress. Each reply goes to [`ConnectionTest::accept`],
/// which gives the next step or says why the connection is not good.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionTest {
    credential: Credential,
    network: Network,
    node: [u8; 33],
    ask: Ask,
    step: Step,
}

impl ConnectionTest {
    /// A test of `credential` for a shop on `network` whose settings name
    /// `node` (`Settings::lightning_node`). `counter` is the first four bytes
    /// of the counter's device id and `test_id` a number not used before for
    /// a test on this node (the time of the test will do), which together
    /// make the label of the test invoice at Core Lightning.
    pub fn new(
        credential: Credential,
        network: Network,
        node: [u8; 33],
        counter: [u8; 4],
        test_id: u64,
    ) -> Self {
        Self {
            credential,
            network,
            node,
            ask: Ask::test(counter, test_id),
            step: Step::ProbeOne,
        }
    }

    fn rule(&self) -> &'static str {
        match self.credential {
            Credential::Lnd(_) => "LN-06",
            Credential::Cln(_) => "LN-09",
        }
    }

    fn probe(&self, which: usize) -> Option<(&'static str, Request)> {
        match &self.credential {
            Credential::Lnd(c) => lnd::probes(c).into_iter().nth(which),
            Credential::Cln(c) => cln::probes(c).into_iter().nth(which),
        }
    }

    fn judge(&self, reply: &Reply) -> Probe {
        match &self.credential {
            Credential::Lnd(_) => lnd::judge_probe(reply),
            Credential::Cln(_) => cln::judge_probe(reply),
        }
    }

    /// The request to send now.
    pub fn request(&self) -> Request {
        let probe = |which| self.probe(which).map(|(_, r)| r);
        match self.step {
            Step::ProbeOne => probe(0),
            Step::ProbeTwo => probe(1),
            Step::Invoice => None,
        }
        .unwrap_or_else(|| self.credential.create_request(&self.ask))
    }

    /// Take the reply to [`ConnectionTest::request`].
    pub fn accept(self, reply: &Reply) -> Result<Next, ConnectionRefused> {
        let which = match self.step {
            Step::ProbeOne => 0,
            Step::ProbeTwo => 1,
            Step::Invoice => return self.finish(reply),
        };
        let Some((call, _)) = self.probe(which) else {
            return Err(ConnectionRefused::ProbeUnclear {
                call: "probe",
                status: reply.status,
                rule: self.rule(),
            });
        };
        let rule = self.rule();
        match self.judge(reply) {
            Probe::Denied => {}
            Probe::Allowed => return Err(ConnectionRefused::CredentialTooStrong { call, rule }),
            Probe::NotFromNode => return Err(ConnectionRefused::RuneNotFromNode),
            Probe::Revoked => return Err(ConnectionRefused::RuneRevoked),
            Probe::Unclear => {
                return Err(ConnectionRefused::ProbeUnclear {
                    call,
                    status: reply.status,
                    rule,
                })
            }
        }
        Ok(Next::More(Self {
            step: if self.step == Step::ProbeOne {
                Step::ProbeTwo
            } else {
                Step::Invoice
            },
            ..self
        }))
    }

    fn finish(self, reply: &Reply) -> Result<Next, ConnectionRefused> {
        let created = match self
            .credential
            .parse_create(reply)
            .map_err(ConnectionRefused::NoInvoice)?
        {
            CreateOutcome::Created(c) => c,
            CreateOutcome::LabelExists => return Err(ConnectionRefused::LabelUsed),
        };
        match check_created(&created, &self.ask, self.network, self.node) {
            Ok(test_invoice) => Ok(Next::Done(Connected { test_invoice })),
            Err(InvoiceRefused::WrongNode) => Err(ConnectionRefused::WrongNode),
            Err(e) => Err(ConnectionRefused::BadInvoice(e)),
        }
    }
}
