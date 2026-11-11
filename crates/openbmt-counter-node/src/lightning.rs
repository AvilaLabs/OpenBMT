//! Lightning at the counter: the driver that asks the shop's own node for
//! an invoice for each sale and asks whether it is paid (CR-112 to CR-116).
//!
//! The driver has no network and no clock. It hands out the next request
//! with a number ([`Lightning::next`]) and takes the answer for that number
//! ([`Lightning::reply`], [`Lightning::failed`]); an answer for any other
//! number is ignored. The app sends the request over HTTPS (CR-114), one at
//! a time. What is asked, and how each reply is read, is
//! `openbmt-lightning` (LN-01 to LN-23); this file only decides when.
//!
//! ## What it asks, and when
//!
//! - For each open sale, one invoice for the sale's current quote: the
//!   amount is the quote's satoshis times 1,000 and the lifetime is
//!   `QUOTE_SECS`. A new quote asks again, with the next attempt, and the
//!   old invoice is let go (CR-112).
//! - While the invoice is live and the sale is open, a lookup every
//!   [`LOOKUP_EVERY_SECS`] seconds.
//! - If no invoice has passed LN-17 within [`INVOICE_WINDOW_SECS`] seconds, or
//!   one fails a check, the sale has no Lightning code and says why (CR-113).
//!
//! ## What it keeps
//!
//! The credential, with the node key it was tested against, is a
//! [`LightningRecord`]: the app seals the bytes and hands them back at every
//! start (CR-115). When the signed settings name another node, or none, or
//! the bytes do not read back, Lightning is off (CR-116).

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use bitcoin::Network;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::QUOTE_SECS;
use openbmt_lightning::bolt11::Checked;
use openbmt_lightning::connect::{ConnectionTest, Next};
use openbmt_lightning::error::{ConnectionRefused, CredentialRefused, ReplyRefused};
use openbmt_lightning::node::check_created;
use openbmt_lightning::{
    Ask, CertPin, ClnCredential, CreateOutcome, Created, Credential, Endpoint, LndCredential,
    NotPaid, PaidFacts, Payment, Reply, Request,
};

/// The magic of the saved credential (version 1).
pub const MAGIC_LIGHTNING_RECORD: [u8; 4] = *b"OBMy";
/// CR-113: seconds after the quote in which the invoice must have come.
pub const INVOICE_WINDOW_SECS: u64 = 6;
/// CR-112: seconds between two lookups of a sale.
pub const LOOKUP_EVERY_SECS: u64 = 2;
/// Seconds a lookup goes on after the invoice's lifetime by the counter's
/// clock, because the node made it a moment after the request was sent.
pub const LOOKUP_GRACE_SECS: u64 = 10;
/// Seconds before a request that has no answer is let go.
pub const REQUEST_GIVE_UP_SECS: u64 = 60;
/// Most attempts at one quote (LN-15).
pub const MAX_ATTEMPTS: u32 = 3;
/// Seconds before a request that failed is handed out again.
pub const RETRY_AFTER_SECS: u64 = 1;
/// Most lines the driver keeps for the node's log between two calls.
pub const MAX_NOTES: usize = 32;

const MAX_SECRET_BYTES: usize = 4096;

fn expiry_secs() -> u32 {
    u32::try_from(QUOTE_SECS).unwrap_or(300)
}

/// What the counter saves of the connection: the credential, and the node
/// key of the signed settings it was tested against (CR-116).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightningRecord {
    /// The credential, limited to invoices (LN-04).
    pub credential: Credential,
    /// The node key the test invoice was signed by.
    pub node: [u8; 33],
}

impl LightningRecord {
    /// The bytes the app seals: the magic, version 1, the kind, the node key,
    /// the address, the pin and the secret.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_LIGHTNING_RECORD, 512);
        out.push(match self.credential {
            Credential::Lnd(_) => 0,
            Credential::Cln(_) => 1,
        });
        out.extend_from_slice(&self.node);
        let host_port = self.credential.endpoint().host_port();
        out.push(u8::try_from(host_port.len()).unwrap_or(u8::MAX));
        out.extend_from_slice(host_port.as_bytes());
        match self.credential.endpoint().pin() {
            CertPin::Der(der) => {
                out.push(0);
                out.extend_from_slice(&u16::try_from(der.len()).unwrap_or(u16::MAX).to_le_bytes());
                out.extend_from_slice(der);
            }
            CertPin::Sha256(d) => {
                out.push(1);
                out.extend_from_slice(d);
            }
        }
        let secret: &[u8] = match &self.credential {
            Credential::Lnd(c) => c.macaroon(),
            Credential::Cln(c) => c.rune().as_bytes(),
        };
        out.extend_from_slice(
            &u16::try_from(secret.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        out.extend_from_slice(secret);
        out
    }

    /// Read the bytes strictly: every length within its limit, the address
    /// and the pin and the secret each valid as the library takes them.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let bad = |why: &'static str| DecodeError::NonCanonical(why);
        let mut r = Reader::framed(bytes, &MAGIC_LIGHTNING_RECORD)?;
        let kind = r.u8()?;
        let node: [u8; 33] = r.array()?;
        let n = usize::from(r.u8()?);
        let host_port = std::str::from_utf8(r.take(n)?)
            .map_err(|_| bad("the address is not UTF-8"))?
            .to_string();
        let pin = match r.u8()? {
            0 => {
                let n = usize::from(r.u16_le()?);
                CertPin::from_der(r.take(n)?).map_err(|_| bad("the certificate is not valid"))?
            }
            1 => CertPin::Sha256(r.array()?),
            _ => return Err(bad("the pin kind is not known")),
        };
        let n = usize::from(r.u16_le()?);
        if n > MAX_SECRET_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let secret = r.take(n)?;
        r.finish()?;
        let endpoint = Endpoint::parse(&host_port, pin).map_err(|_| bad("the address"))?;
        let credential = match kind {
            0 => Credential::Lnd(
                LndCredential::new(endpoint, secret).map_err(|_| bad("the macaroon"))?,
            ),
            1 => {
                let rune = std::str::from_utf8(secret).map_err(|_| bad("the rune"))?;
                Credential::Cln(ClnCredential::new(endpoint, rune).map_err(|_| bad("the rune"))?)
            }
            _ => return Err(bad("the node kind is not known")),
        };
        Ok(Self { credential, node })
    }
}

/// Why the connection to the node failed before any answer was read: what
/// the app's transport reports (CR-114).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LnTransport {
    /// The node showed another certificate than the pinned one.
    PinRefused,
    /// No connection could be made.
    Unreachable,
    /// The connect or the read took too long.
    TimedOut,
    /// The body is longer than the app takes.
    TooBig,
    /// The node answered with a redirect.
    Redirected,
}

impl LnTransport {
    /// Every one of them, for the list of words.
    pub const ALL: [LnTransport; 5] = [
        Self::PinRefused,
        Self::Unreachable,
        Self::TimedOut,
        Self::TooBig,
        Self::Redirected,
    ];

    /// The name of the variant.
    pub fn name(&self) -> &'static str {
        match self {
            Self::PinRefused => "PinRefused",
            Self::Unreachable => "Unreachable",
            Self::TimedOut => "TimedOut",
            Self::TooBig => "TooBig",
            Self::Redirected => "Redirected",
        }
    }

    /// What the owner is told when the test of a connection fails this way.
    pub fn words(&self) -> Words {
        let (heading, what, next) = match self {
            Self::PinRefused => (
                "Node certificate differs",
                "The node showed another certificate than the one given, so the counter did not send anything to it.",
                "Copy the node's certificate again from the node itself, then try again.",
            ),
            Self::Unreachable => (
                "Lightning node not reached",
                "The counter could not make a connection to the Lightning node at this address.",
                "Check the address and that the node is running and reachable from here, then try again.",
            ),
            Self::TimedOut => (
                "Lightning node too slow",
                "The Lightning node did not answer in time.",
                "Check that the node is running and reachable from here, then try again.",
            ),
            Self::TooBig => (
                "Node answer too large",
                "The Lightning node sent more than the counter takes in one answer, so it was thrown away.",
                "Check that the address is the node's own REST address, then try again.",
            ),
            Self::Redirected => (
                "Node sent us elsewhere",
                "The address answered with a redirect, which the counter never follows.",
                "Use the node's own REST address, not a web page or a proxy in front of it.",
            ),
        };
        Words::new(Tone::Stop, heading, what, Who::Owner, next).code("CR-114")
    }

    /// The same, for the cashier at a sale.
    pub fn sale_words(&self) -> Words {
        let w = Words::new(
            Tone::Stop,
            "Lightning not available",
            "Lightning is not available for this sale. The on-chain code still works.",
            Who::Cashier,
            "Take another kind of payment if the customer has only Lightning, and tell the owner.",
        )
        .code("CR-113");
        w.detail(self.words().what)
    }
}

/// Why a saved credential is not in use (CR-116).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightningOff {
    /// The signed settings name no node.
    NotNamed,
    /// The signed settings name another node than the one the credential
    /// was tested against.
    NodeChanged,
    /// The saved bytes do not read back.
    Unreadable,
}

impl LightningOff {
    /// Every one of them, for the list of words.
    pub const ALL: [LightningOff; 3] = [Self::NotNamed, Self::NodeChanged, Self::Unreadable];

    /// The name of the variant.
    pub fn name(&self) -> &'static str {
        match self {
            Self::NotNamed => "NotNamed",
            Self::NodeChanged => "NodeChanged",
            Self::Unreadable => "Unreadable",
        }
    }

    /// What the owner is told, on the status line.
    pub fn words(&self) -> Words {
        let (heading, what, next) = match self {
            Self::NotNamed => (
                "Lightning is off",
                "The shop's settings no longer name a Lightning node, so the counter does not use the saved connection.",
                "Name the node in the shop's settings, then set the Lightning connection up again.",
            ),
            Self::NodeChanged => (
                "Lightning turned off",
                "The shop's settings now name another Lightning node than the one this connection was tested with.",
                "Set the Lightning connection up again for the node the settings name.",
            ),
            Self::Unreadable => (
                "Lightning connection lost",
                "The saved Lightning connection could not be read back, so it is not used.",
                "Remove it and set the Lightning connection up again.",
            ),
        };
        Words::new(Tone::Stop, heading, what, Who::Owner, next).code("CR-116")
    }
}

/// Why the setup did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupRefused {
    /// The signed settings name no node (CR-115): nothing is tested or kept.
    NotNamed,
    /// What was typed or scanned is not a credential (LN-04 to LN-12).
    Credential(CredentialRefused),
    /// The test of the connection says no (LN-20).
    Connection(ConnectionRefused),
    /// The request did not get an answer (CR-114).
    Transport(LnTransport),
    /// The setup the answer belongs to is not the one under way.
    NoSetup,
}

impl SetupRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NotNamed | Self::NoSetup => "CR-115",
            Self::Credential(e) => e.rule(),
            Self::Connection(e) => e.rule(),
            Self::Transport(_) => "CR-114",
        }
    }

    /// What the owner is told.
    pub fn words(&self) -> Words {
        match self {
            Self::NotNamed => Words::new(
                Tone::Stop,
                "No Lightning node named",
                "The owner has not named the shop's Lightning node yet, so nothing was tested and nothing was saved.",
                Who::Owner,
                "Name the node in the shop's settings with the owner's phone, then set Lightning up here.",
            )
            .code("CR-115"),
            Self::Credential(e) => e.words(),
            Self::Connection(e) => e.words(),
            Self::Transport(t) => t.words(),
            Self::NoSetup => Words::new(
                Tone::Stop,
                "Lightning setup not running",
                "There is no Lightning test under way for this answer, so it was dropped.",
                Who::Manager,
                "Start the Lightning setup again from the first step.",
            )
            .code("CR-115"),
        }
    }
}

impl fmt::Display for SetupRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotNamed => write!(f, "CR-115: the settings name no Lightning node"),
            Self::Credential(e) => write!(f, "{e}"),
            Self::Connection(e) => write!(f, "{e}"),
            Self::Transport(t) => write!(f, "CR-114: {}", t.name()),
            Self::NoSetup => write!(f, "CR-115: no Lightning setup is under way"),
        }
    }
}

impl std::error::Error for SetupRefused {}

/// What the last contact with the node showed (CR-116).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contact {
    /// Nothing asked yet.
    Unknown,
    Good,
    Failed(LnTransport),
    /// The node said no to the key itself (HTTP 401 or 403).
    KeyRefused,
}

/// The line of the counter's status about Lightning (CR-116).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightningStatus {
    /// Whether sales get a Lightning code now.
    pub ready: bool,
    /// What to show.
    pub words: Words,
}

/// What the screen of a sale shows of Lightning (CR-113).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaleLightning {
    /// No credential in use, or the sale is not one the driver follows: no
    /// Lightning line.
    Off,
    /// The invoice has been asked for and has not come.
    Waiting,
    /// The invoice passed LN-17: the code carries it.
    Invoice(Checked),
    /// There is none for this sale, and why.
    Unavailable(Words),
}

/// One request for the app to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnRequest {
    /// The number the answer comes back with.
    pub id: u64,
    /// `host:port` of the node.
    pub host_port: String,
    /// What to send.
    pub request: Request,
}

/// What an answer led to that the node must act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LnEvent {
    /// Nothing for the node to do.
    Nothing,
    /// The node says the invoice of this sale is paid (LN-14).
    Paid {
        /// The sale.
        index: u32,
        /// What the node says of the payment.
        facts: PaidFacts,
    },
}

/// What a setup answer led to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupStep {
    /// Send this next.
    More(LnRequest),
    /// The connection is good and in use. These are the bytes to seal.
    Done(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// The invoice must be asked for (or asked for again, same attempt).
    Want { retry_at: u64 },
    /// The request for it is out.
    Asked,
    /// Core Lightning said the label is used: ask for that invoice.
    WantLabel { retry_at: u64 },
    /// The request for the invoice by label is out.
    AskedLabel,
    /// The invoice passed LN-17.
    Have(Checked),
    /// There is none for this quote.
    Unavailable(Words),
    /// The sale is over for Lightning: paid, or the invoice ran out.
    Ended,
}

#[derive(Debug, Clone)]
struct SaleLn {
    attempt: u32,
    /// Counts the quotes, so that a late answer for an old one is dropped.
    round: u64,
    sats: u64,
    window_start: u64,
    ask: Option<Ask>,
    phase: Phase,
    last_lookup: Option<u64>,
    last_failure: Option<LnTransport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Out {
    Create { index: u32, round: u64 },
    Label { index: u32, round: u64 },
    Lookup { index: u32, round: u64 },
}

struct Outstanding {
    id: u64,
    sent: u64,
    what: Out,
}

struct Setup {
    id: u64,
    test: ConnectionTest,
    credential: Credential,
    node: [u8; 33],
}

struct Active {
    record: LightningRecord,
    contact: Contact,
}

/// The driver. See the module docs.
pub struct Lightning {
    network: Network,
    counter: [u8; 4],
    /// `Settings::lightning_node` of the settings the counter runs on.
    settings_node: Option<[u8; 33]>,
    active: Option<Active>,
    /// The state of a saved credential that is not in use (CR-116).
    off: Option<LightningOff>,
    setup: Option<Setup>,
    sales: BTreeMap<u32, SaleLn>,
    out: Option<Outstanding>,
    next_id: u64,
    next_round: u64,
    notes: VecDeque<String>,
}

impl Lightning {
    /// A driver for a counter on `network` whose first four id bytes are
    /// `counter`, on settings that name `settings_node`, with the saved
    /// bytes of a credential if there are any (CR-116).
    pub fn new(
        network: Network,
        counter: [u8; 4],
        settings_node: Option<[u8; 33]>,
        saved: Option<&[u8]>,
    ) -> Self {
        let mut me = Self {
            network,
            counter,
            settings_node,
            active: None,
            off: None,
            setup: None,
            sales: BTreeMap::new(),
            out: None,
            next_id: 1,
            next_round: 1,
            notes: VecDeque::new(),
        };
        if let Some(bytes) = saved {
            match LightningRecord::from_bytes(bytes) {
                Err(e) => {
                    me.off = Some(LightningOff::Unreadable);
                    me.note(format!(
                        "the saved Lightning connection does not read back: {e}"
                    ));
                }
                Ok(record) => match settings_node {
                    None => me.off = Some(LightningOff::NotNamed),
                    Some(n) if n != record.node => me.off = Some(LightningOff::NodeChanged),
                    Some(_) => {
                        me.active = Some(Active {
                            record,
                            contact: Contact::Unknown,
                        });
                    }
                },
            }
            if let Some(off) = me.off {
                me.note(format!("Lightning is off: {}", off.words().heading));
            }
        }
        me
    }

    fn note(&mut self, line: String) {
        if self.notes.len() >= MAX_NOTES {
            self.notes.pop_front();
        }
        self.notes.push_back(line);
    }

    /// The lines for the node's log since the last call.
    pub fn take_notes(&mut self) -> Vec<String> {
        self.notes.drain(..).collect()
    }

    /// Whether sales get a Lightning code (the credential is in use).
    pub fn is_on(&self) -> bool {
        self.active.is_some()
    }

    /// CR-116: the status line, only when a credential is saved.
    pub fn status(&self) -> Option<LightningStatus> {
        if let Some(off) = self.off {
            return Some(LightningStatus {
                ready: false,
                words: off.words(),
            });
        }
        let active = self.active.as_ref()?;
        let (ready, words) = match active.contact {
            Contact::Unknown | Contact::Good => (true, ready_words()),
            Contact::Failed(t) => (false, not_reached_words().detail(t.words().what)),
            Contact::KeyRefused => (false, key_refused_words()),
        };
        Some(LightningStatus { ready, words })
    }

    /// Whether a credential is saved, in use or not.
    pub fn has_credential(&self) -> bool {
        self.active.is_some() || self.off.is_some()
    }

    /// CR-115: remove the credential at once: nothing is in use or kept.
    pub fn remove(&mut self) {
        self.active = None;
        self.off = None;
        self.setup = None;
        self.out = None;
        self.sales.clear();
    }

    // Setup (CR-115).

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Begin the test of a connection (LN-20) against the node key of the
    /// signed settings. If the settings name no node, nothing is tested.
    pub fn setup_begin(
        &mut self,
        credential: Credential,
        now: u64,
    ) -> Result<LnRequest, SetupRefused> {
        let node = self.settings_node.ok_or(SetupRefused::NotNamed)?;
        let test = ConnectionTest::new(credential.clone(), self.network, node, self.counter, now);
        let id = self.take_id();
        let request = LnRequest {
            id,
            host_port: credential.endpoint().host_port(),
            request: test.request(),
        };
        self.setup = Some(Setup {
            id,
            test,
            credential,
            node,
        });
        Ok(request)
    }

    fn setup_reply(&mut self, reply: &Reply) -> Result<SetupStep, SetupRefused> {
        let setup = self.setup.take().ok_or(SetupRefused::NoSetup)?;
        let Setup {
            test,
            credential,
            node,
            ..
        } = setup;
        match test.accept(reply).map_err(SetupRefused::Connection)? {
            Next::More(test) => {
                let id = self.take_id();
                let request = LnRequest {
                    id,
                    host_port: credential.endpoint().host_port(),
                    request: test.request(),
                };
                self.setup = Some(Setup {
                    id,
                    test,
                    credential,
                    node,
                });
                Ok(SetupStep::More(request))
            }
            Next::Done(_) => {
                let record = LightningRecord { credential, node };
                let bytes = record.to_bytes();
                self.active = Some(Active {
                    record,
                    contact: Contact::Good,
                });
                self.off = None;
                self.sales.clear();
                self.out = None;
                Ok(SetupStep::Done(bytes))
            }
        }
    }

    // Sales (CR-112, CR-113).

    /// A sale was opened, or restored live, with `sats` to pay.
    pub fn sale_opened(&mut self, index: u32, sats: u64, now: u64) {
        if self.active.is_none() {
            return;
        }
        let round = self.next_round;
        self.next_round += 1;
        self.sales.insert(
            index,
            SaleLn {
                attempt: 0,
                round,
                sats,
                window_start: now,
                ask: None,
                phase: Phase::Want { retry_at: now },
                last_lookup: None,
                last_failure: None,
            },
        );
    }

    /// CR-112: the sale has a new quote: the invoice for the old one is let
    /// go and a new one is asked for, with the next attempt.
    pub fn sale_requoted(&mut self, index: u32, sats: u64, now: u64) {
        if self.active.is_none() {
            return;
        }
        let round = self.next_round;
        self.next_round += 1;
        let attempt = self.sales.get(&index).map_or(0, |s| s.attempt + 1);
        self.sales.insert(
            index,
            SaleLn {
                attempt,
                round,
                sats,
                window_start: now,
                ask: None,
                phase: Phase::Want { retry_at: now },
                last_lookup: None,
                last_failure: None,
            },
        );
    }

    /// The sale is over (paid another way, cancelled, expired or final).
    pub fn sale_closed(&mut self, index: u32) {
        self.sales.remove(&index);
    }

    /// What the screen of the sale shows of Lightning (CR-113).
    pub fn sale_view(&mut self, index: u32, now: u64) -> SaleLightning {
        self.settle_windows(now);
        match self.sales.get(&index).map(|s| &s.phase) {
            None | Some(Phase::Ended) => SaleLightning::Off,
            Some(
                Phase::Want { .. } | Phase::Asked | Phase::WantLabel { .. } | Phase::AskedLabel,
            ) => SaleLightning::Waiting,
            Some(Phase::Have(c)) => SaleLightning::Invoice(c.clone()),
            Some(Phase::Unavailable(w)) => SaleLightning::Unavailable(w.clone()),
        }
    }

    /// CR-113: a sale with no invoice [`INVOICE_WINDOW_SECS`] seconds after
    /// its quote has none.
    fn settle_windows(&mut self, now: u64) {
        let mut lines = Vec::new();
        for (index, s) in &mut self.sales {
            let waiting = matches!(
                s.phase,
                Phase::Want { .. } | Phase::Asked | Phase::WantLabel { .. } | Phase::AskedLabel
            );
            if waiting && now >= s.window_start.saturating_add(INVOICE_WINDOW_SECS) {
                let words = match s.last_failure {
                    Some(t) => t.sale_words(),
                    None => no_invoice_words("the node did not answer in time"),
                };
                lines.push(format!("sale {index}: no Lightning invoice in time"));
                s.phase = Phase::Unavailable(words);
            }
        }
        for line in lines {
            self.note(line);
        }
    }

    /// The next request to send, if one is due. At most one is out at a
    /// time. `live` says whether a sale is open and unpaid.
    pub fn next(&mut self, now: u64, live: impl Fn(u32) -> bool) -> Option<LnRequest> {
        self.settle_windows(now);
        if let Some(o) = &self.out {
            if now < o.sent.saturating_add(REQUEST_GIVE_UP_SECS) {
                return None;
            }
            self.out = None;
        }
        let credential = self.active.as_ref()?.record.credential.clone();
        let indexes: Vec<u32> = self.sales.keys().copied().collect();
        for index in indexes {
            if !live(index) {
                self.sales.remove(&index);
                continue;
            }
            let counter = self.counter;
            let id = self.next_id;
            let Some(s) = self.sales.get_mut(&index) else {
                continue;
            };
            let round = s.round;
            let (request, what) = match &s.phase {
                Phase::Want { retry_at } if now >= *retry_at => {
                    let ask = match &s.ask {
                        Some(a) => a.clone(),
                        None => {
                            let msat = s.sats.saturating_mul(1_000);
                            match Ask::sale(
                                counter,
                                u64::from(index),
                                s.attempt,
                                msat,
                                expiry_secs(),
                            ) {
                                Ok(a) => a,
                                Err(_) => {
                                    s.phase = Phase::Unavailable(no_invoice_words(
                                        "the amount cannot be asked of the node",
                                    ));
                                    continue;
                                }
                            }
                        }
                    };
                    let request = credential.create_request(&ask);
                    s.ask = Some(ask);
                    s.phase = Phase::Asked;
                    (request, Out::Create { index, round })
                }
                Phase::WantLabel { retry_at } if now >= *retry_at => {
                    let Some(request) = s.ask.as_ref().and_then(|a| credential.label_request(a))
                    else {
                        s.phase = Phase::Unavailable(no_invoice_words(
                            "the node has no invoice with the label",
                        ));
                        continue;
                    };
                    s.phase = Phase::AskedLabel;
                    (request, Out::Label { index, round })
                }
                Phase::Have(c) => {
                    let due = s
                        .last_lookup
                        .is_none_or(|t| now >= t.saturating_add(LOOKUP_EVERY_SECS));
                    let lives = now
                        < s.window_start
                            .saturating_add(QUOTE_SECS)
                            .saturating_add(LOOKUP_GRACE_SECS)
                            .saturating_add(INVOICE_WINDOW_SECS);
                    if !lives {
                        s.phase = Phase::Ended;
                        continue;
                    }
                    if !due {
                        continue;
                    }
                    s.last_lookup = Some(now);
                    (
                        credential.lookup_request(&c.payment_hash),
                        Out::Lookup { index, round },
                    )
                }
                _ => continue,
            };
            self.next_id = id.saturating_add(1);
            self.out = Some(Outstanding {
                id,
                sent: now,
                what,
            });
            return Some(LnRequest {
                id,
                host_port: credential.endpoint().host_port(),
                request,
            });
        }
        None
    }

    /// The certificate the node must show for the request with this number
    /// (CR-114), if the request is the one out or the setup's.
    pub fn pin_for(&self, id: u64) -> Option<&CertPin> {
        if let Some(o) = &self.out {
            if o.id == id {
                return self
                    .active
                    .as_ref()
                    .map(|a| a.record.credential.endpoint().pin());
            }
        }
        match &self.setup {
            Some(s) if s.id == id => Some(s.credential.endpoint().pin()),
            _ => None,
        }
    }

    /// Whether `id` is the number of the setup's request.
    pub fn is_setup(&self, id: u64) -> bool {
        self.setup.as_ref().is_some_and(|s| s.id == id)
    }

    /// The answer for request `id`. A number that is not the one out (or the
    /// setup's) is ignored.
    pub fn reply(&mut self, id: u64, reply: &Reply) -> LnEvent {
        let Some(what) = self.take_out(id) else {
            return LnEvent::Nothing;
        };
        if let Some(a) = &mut self.active {
            a.contact = if matches!(reply.status, 401 | 403) {
                Contact::KeyRefused
            } else {
                Contact::Good
            };
        }
        self.answered(what, reply)
    }

    /// The answer of a setup request: the next step, or why it stopped.
    pub fn setup_answer(&mut self, id: u64, reply: &Reply) -> Result<SetupStep, SetupRefused> {
        if !self.is_setup(id) {
            return Err(SetupRefused::NoSetup);
        }
        self.setup_reply(reply)
    }

    /// The request `id` got no answer: the transport says why (CR-114).
    pub fn failed(&mut self, id: u64, why: LnTransport, now: u64) {
        let Some(what) = self.take_out(id) else {
            return;
        };
        if let Some(a) = &mut self.active {
            a.contact = Contact::Failed(why);
        }
        self.note(format!(
            "the Lightning node was not reached: {}",
            why.name()
        ));
        let (index, round) = match what {
            Out::Create { index, round }
            | Out::Label { index, round }
            | Out::Lookup { index, round } => (index, round),
        };
        let Some(s) = self.sales.get_mut(&index) else {
            return;
        };
        if s.round != round {
            return;
        }
        s.last_failure = Some(why);
        // LN-15: after no answer, the same request is sent again, with the
        // same attempt, while the window lasts.
        let retry_at = now.saturating_add(RETRY_AFTER_SECS);
        match (&what, &s.phase) {
            (Out::Create { .. }, Phase::Asked) => s.phase = Phase::Want { retry_at },
            (Out::Label { .. }, Phase::AskedLabel) => s.phase = Phase::WantLabel { retry_at },
            _ => {}
        }
    }

    /// The setup request `id` got no answer.
    pub fn setup_failed(&mut self, id: u64, why: LnTransport) -> SetupRefused {
        if self.is_setup(id) {
            self.setup = None;
            SetupRefused::Transport(why)
        } else {
            SetupRefused::NoSetup
        }
    }

    fn take_out(&mut self, id: u64) -> Option<Out> {
        match &self.out {
            Some(o) if o.id == id => self.out.take().map(|o| o.what),
            _ => None,
        }
    }

    fn answered(&mut self, what: Out, reply: &Reply) -> LnEvent {
        let Some(active) = &self.active else {
            return LnEvent::Nothing;
        };
        let credential = active.record.credential.clone();
        let (network, node) = (self.network, active.record.node);
        let (index, round) = match what {
            Out::Create { index, round }
            | Out::Label { index, round }
            | Out::Lookup { index, round } => (index, round),
        };
        let mut lines = Vec::new();
        let mut event = LnEvent::Nothing;
        if let Some(s) = self.sales.get_mut(&index) {
            if s.round == round {
                match what {
                    Out::Create { .. } if s.phase == Phase::Asked => {
                        let made = credential.parse_create(reply);
                        match made {
                            Ok(CreateOutcome::Created(c)) => {
                                judge_created(s, &c, network, node, &mut lines, index)
                            }
                            Ok(CreateOutcome::LabelExists) => {
                                s.phase = Phase::WantLabel { retry_at: 0 };
                            }
                            Err(e) => {
                                lines.push(format!("sale {index}: no Lightning invoice: {e}"));
                                s.phase = Phase::Unavailable(refused_words(&e));
                            }
                        }
                    }
                    Out::Label { .. } if s.phase == Phase::AskedLabel => {
                        let existing = s.ask.as_ref().map(|a| credential.parse_existing(reply, a));
                        match existing {
                            Some(Ok(c)) => judge_created(s, &c, network, node, &mut lines, index),
                            _ => {
                                // LN-15: the label is not this sale's: the
                                // next attempt has a new label.
                                s.attempt += 1;
                                s.ask = None;
                                if s.attempt >= MAX_ATTEMPTS {
                                    s.phase = Phase::Unavailable(no_invoice_words(
                                        "the label belongs to another invoice",
                                    ));
                                } else {
                                    s.phase = Phase::Want { retry_at: 0 };
                                }
                            }
                        }
                    }
                    Out::Lookup { .. } => {
                        if let Phase::Have(c) = &s.phase {
                            let asked = s.ask.as_ref().map_or(0, Ask::amount_msat);
                            match credential.parse_lookup(reply, &c.payment_hash, asked) {
                                Ok(Payment::Paid(facts)) => {
                                    event = LnEvent::Paid { index, facts };
                                }
                                Ok(Payment::NotPaid(
                                    NotPaid::Expired | NotPaid::Cancelled | NotPaid::NotFound,
                                )) => {
                                    lines.push(format!(
                                        "sale {index}: the Lightning invoice is over"
                                    ));
                                    s.phase = Phase::Ended;
                                }
                                Ok(Payment::NotPaid(NotPaid::TooLittle { asked, received })) => {
                                    lines.push(format!(
                                        "sale {index}: the node settled {received} msat for {asked} msat asked"
                                    ));
                                }
                                Ok(Payment::NotPaid(_)) => {}
                                Err(e) => lines.push(format!("sale {index}: lookup refused: {e}")),
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        for line in lines {
            self.note(line);
        }
        event
    }

    /// The sale is paid (by the counter's own judgment, CR-109): stop asking.
    pub fn paid(&mut self, index: u32) {
        if let Some(s) = self.sales.get_mut(&index) {
            s.phase = Phase::Ended;
        }
    }
}

fn judge_created(
    s: &mut SaleLn,
    created: &Created,
    network: Network,
    node: [u8; 33],
    lines: &mut Vec<String>,
    index: u32,
) {
    let Some(ask) = &s.ask else {
        return;
    };
    match check_created(created, ask, network, node) {
        Ok(checked) => s.phase = Phase::Have(checked),
        Err(e) => {
            lines.push(format!("sale {index}: the Lightning invoice fails: {e}"));
            let words = e.words();
            s.phase = Phase::Unavailable(unavailable(&words, e.rule()));
        }
    }
}

/// The line that says Lightning is not available for this sale, with the
/// reason, in the cashier's words of LN-22, under the details (CR-113).
fn unavailable(reason: &Words, rule: &'static str) -> Words {
    let mut detail = format!("{}. {}", reason.heading, reason.what);
    if let Some(d) = &reason.detail {
        detail.push_str(" (");
        detail.push_str(d);
        detail.push(')');
    }
    Words::new(
        Tone::Stop,
        "Lightning not available",
        "Lightning is not available for this sale. The on-chain code still works.",
        Who::Cashier,
        "Take another kind of payment if the customer has only Lightning, and tell the owner.",
    )
    .code(rule_code(rule))
    .detail(detail)
}

fn rule_code(rule: &'static str) -> &'static str {
    if rule.starts_with("LN-") {
        rule
    } else {
        "CR-113"
    }
}

fn refused_words(e: &ReplyRefused) -> Words {
    unavailable(&e.words(), e.rule())
}

fn no_invoice_words(why: &str) -> Words {
    Words::new(
        Tone::Stop,
        "Lightning not available",
        "Lightning is not available for this sale. The on-chain code still works.",
        Who::Cashier,
        "Take another kind of payment if the customer has only Lightning, and tell the owner.",
    )
    .code("CR-113")
    .detail(why)
}

/// One value of each of the words this file gives, for SC-12.
pub fn all_words() -> Vec<(String, Words)> {
    let mut all = Vec::new();
    for t in LnTransport::ALL {
        all.push((format!("LnTransport::{}", t.name()), t.words()));
    }
    for off in LightningOff::ALL {
        all.push((format!("LightningOff::{}", off.name()), off.words()));
    }
    all.push((
        "SetupRefused::NotNamed".to_string(),
        SetupRefused::NotNamed.words(),
    ));
    all.push((
        "SetupRefused::NoSetup".to_string(),
        SetupRefused::NoSetup.words(),
    ));
    all.push(("Lightning::Ready".to_string(), ready_words()));
    all.push((
        "Lightning::Unavailable".to_string(),
        no_invoice_words("the node did not answer in time"),
    ));
    all.push(("Lightning::NotReached".to_string(), not_reached_words()));
    all.push(("Lightning::KeyRefused".to_string(), key_refused_words()));
    all
}

fn ready_words() -> Words {
    Words::new(
        Tone::Good,
        "Lightning ready",
        "The counter can take Lightning payments through the shop's own node.",
        Who::Nobody,
        "Nothing to do.",
    )
    .code("CR-116")
}

fn not_reached_words() -> Words {
    Words::new(
        Tone::Stop,
        "Lightning node not reached",
        "The counter could not reach the shop's Lightning node the last time, so sales show the on-chain code only.",
        Who::Owner,
        "Check that the node is running and reachable from here. The counter tries again with each sale.",
    )
    .code("CR-116")
}

fn key_refused_words() -> Words {
    Words::new(
        Tone::Stop,
        "Lightning node refused key",
        "The shop's Lightning node refused the counter's key the last time, so sales show the on-chain code only.",
        Who::Owner,
        "Make a new limited key on the node, remove this connection and set it up again.",
    )
    .code("CR-116")
}
