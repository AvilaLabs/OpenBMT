//! What a person is told about a Lightning payment, a refusal or a setup
//! (`docs/SCREEN-RULES.md`, SC-01 to SC-12; LN-22).
//!
//! The words of a refusal at a sale are for the cashier, who cannot fix the
//! node: they say to take another kind of payment and what to tell the
//! owner. The words of a setup are for the owner, who can. The rule number
//! and the node's own text are in the details only.

use openbmt_core::say::{Quoted, Tone, Who, Words};

use crate::error::{
    AskRefused, ConnectionRefused, CredentialRefused, InvoiceRefused, ReplyRefused, UriRefused,
};
use crate::invoice::{NotPaid, PaidFacts};

const REPORT: &str = "Nothing was changed. Take another kind of payment, and tell the owner that the Lightning node gave an answer the counter does not accept.";
const NEW_RUNE: &str =
    "Make a rune that allows only the invoice and listinvoices methods, and use that.";
const NEW_MACAROON: &str =
    "Make a macaroon with lncli bakemacaroon invoices:read invoices:write, and use that.";
const TELL_OWNER: &str = "Take another kind of payment. Tell the owner: the Lightning node is not working as the counter needs.";

fn setup(heading: &str, what: &str, next: &str, code: &'static str, detail: String) -> Words {
    Words::new(Tone::Stop, heading, what, Who::Owner, next)
        .code(code)
        .detail(detail)
}

fn sale(heading: &str, what: &str, next: &str, code: &'static str, detail: String) -> Words {
    Words::new(Tone::Stop, heading, what, Who::Cashier, next)
        .code(code)
        .detail(detail)
}

impl CredentialRefused {
    /// What the owner is told when a credential or an address is refused.
    pub fn words(&self) -> Words {
        let (heading, what, next) = match self {
            Self::BadAddress => (
                "Node address not valid",
                "The address of the Lightning node is not a host name or number followed by a port.",
                "Read the address again from the node's own screen, then type it in again.",
            ),
            Self::BadConnectString => (
                "Connect text not valid",
                "The text copied from the node is not an lndconnect address with a certificate and a macaroon.",
                "Copy it again from the node's screen, whole, then try again.",
            ),
            Self::NoCertificate => (
                "Node certificate missing",
                "The counter only talks to a node whose certificate it was given, and none came with this address.",
                "Copy the node's certificate file as well, or use the connect text that includes it.",
            ),
            Self::BadCertificate => (
                "Node certificate not valid",
                "The certificate is not a certificate file this counter can read.",
                "Copy the node's certificate file again, whole, then try again.",
            ),
            Self::BadFingerprint => (
                "Fingerprint not valid",
                "The fingerprint is not 64 letters and digits, in pairs or not.",
                "Copy the fingerprint of the node's certificate again, whole.",
            ),
            Self::BadMacaroon => (
                "Macaroon not valid",
                "The macaroon is empty, too long or not a macaroon.",
                NEW_MACAROON,
            ),
            Self::RuneNotBase64 => (
                "Rune not readable",
                "The rune is not written as Core Lightning writes runes, or it is cut short.",
                "Copy the rune again, whole, from where the node made it.",
            ),
            Self::RuneUnrestricted => (
                "Rune has no limits",
                "This rune can do everything on the node, and the counter only takes one limited to invoices.",
                NEW_RUNE,
            ),
            Self::RuneNotText => (
                "Rune limits not readable",
                "The limits written in the rune are not text this counter can read.",
                NEW_RUNE,
            ),
            Self::RuneSyntax(_) => (
                "Rune written in a new way",
                "The rune's limits use a form this counter does not know, so it cannot tell what the rune allows.",
                NEW_RUNE,
            ),
            Self::RuneMixesMethod => (
                "Rune limit too loose",
                "One limit of this rune lists the method together with other conditions, so it may allow more than invoices.",
                NEW_RUNE,
            ),
            Self::RuneAllowsMethod(_) => (
                "Rune allows too much",
                "This rune allows more on the node than making and reading invoices.",
                NEW_RUNE,
            ),
            Self::RuneLacksMethod => (
                "Rune not limited to invoices",
                "This rune does not limit the node's methods to invoice and listinvoices, or leaves one of them out.",
                NEW_RUNE,
            ),
        };
        let words = setup(heading, what, next, self.rule(), self.to_string());
        match self {
            Self::RuneAllowsMethod(m) => words.quoted(Quoted::new("The rune allows", m)),
            _ => words,
        }
    }
}

impl ReplyRefused {
    /// What the cashier is told when the node's reply cannot be used.
    pub fn words(&self) -> Words {
        let (heading, what, next) = match self {
            Self::NodeSaid { .. } => (
                "Node reported a fault",
                "The Lightning node answered with an error instead of doing what the counter asked.",
                TELL_OWNER,
            ),
            Self::TooLarge => (
                "Node answer too big",
                "The Lightning node sent an answer bigger than the counter reads, so none of it was used.",
                REPORT,
            ),
            Self::Unreadable => (
                "Node answer not understood",
                "The Lightning node sent an answer that is not written as expected, so none of it was used.",
                REPORT,
            ),
            Self::Missing(_) => (
                "Node answer incomplete",
                "The Lightning node left out something the counter needs, so none of the answer was used.",
                REPORT,
            ),
            Self::WrongType(_) => (
                "Node answer in a new form",
                "The Lightning node sent something in a form the counter does not know, so none of it was used.",
                REPORT,
            ),
            Self::OutOfRange(_) => (
                "Node number out of range",
                "The Lightning node sent a number that is negative, too large or not whole, so none of it was used.",
                REPORT,
            ),
            Self::OtherInvoice => (
                "Answer is for another sale",
                "The Lightning node answered about a different payment than the one the counter asked about.",
                REPORT,
            ),
            Self::Several => (
                "Node sent several invoices",
                "The Lightning node sent more than one invoice where the counter asked for one.",
                REPORT,
            ),
            Self::LabelTaken => (
                "Node has an older invoice",
                "The node already has an invoice under this sale's name that is not the one this sale asked for.",
                "Try Lightning again, which uses a new name. If it repeats, take another kind of payment.",
            ),
            Self::LabelGone => (
                "Node lost an invoice",
                "The node said an invoice with this sale's name exists, and then could not find it.",
                "Try Lightning again. If it repeats, take another kind of payment.",
            ),
        };
        let words = sale(heading, what, next, self.rule(), self.to_string());
        match self {
            Self::NodeSaid { message, .. } if !message.is_empty() => {
                words.quoted(Quoted::new("The node said", message))
            }
            _ => words,
        }
    }
}

impl InvoiceRefused {
    /// What the cashier is told when the invoice the node made is refused.
    pub fn words(&self) -> Words {
        let (heading, what) = match self {
            Self::TooLong => (
                "Invoice too long",
                "The invoice the node made is longer than the counter reads, so it was not shown.",
            ),
            Self::MixedCase => (
                "Invoice mixes letter cases",
                "The invoice the node made mixes capital and small letters, so it was not shown.",
            ),
            Self::NotAnInvoice => (
                "Not a Lightning invoice",
                "What the node sent as an invoice is not a Lightning invoice, so it was not shown.",
            ),
            Self::UnknownNetwork => (
                "Invoice for unknown network",
                "The invoice names a bitcoin network the counter does not know, so it was not shown.",
            ),
            Self::BadAmount => (
                "Invoice amount not valid",
                "The amount written in the invoice is not written as Lightning writes amounts, so it was not shown.",
            ),
            Self::Truncated => (
                "Invoice cut short",
                "The invoice ends in the middle of a part, so it was not shown.",
            ),
            Self::BadField(_) => (
                "Invoice part not valid",
                "A part of the invoice is of the wrong length or holds what it may not, so it was not shown.",
            ),
            Self::DuplicateField(_) => (
                "Invoice repeats a part",
                "The invoice has a part twice that may be there once, so it was not shown.",
            ),
            Self::MissingField(_) => (
                "Invoice lacks a part",
                "The invoice leaves out a part the counter needs, so it was not shown.",
            ),
            Self::BadSignature => (
                "Invoice signature not valid",
                "The signature of the invoice does not give a signer, so it was not shown.",
            ),
            Self::WrongNetwork => (
                "Invoice for another network",
                "The invoice is for a different bitcoin network than this shop uses, so it was not shown.",
            ),
            Self::NoAmount => (
                "Invoice names no amount",
                "The invoice leaves the amount open, so the customer could pay any amount. It was not shown.",
            ),
            Self::WrongAmount { .. } => (
                "Invoice amount is wrong",
                "The invoice is for a different amount than the sale, so it was not shown.",
            ),
            Self::WrongPaymentHash => (
                "Invoice does not match node",
                "The invoice is for a different payment than the one the node reported, so it was not shown.",
            ),
            Self::WrongExpiry { .. } => (
                "Invoice time is wrong",
                "The invoice lasts for a different time than the sale's price does, so it was not shown.",
            ),
            Self::WrongDescription => (
                "Invoice text is wrong",
                "The invoice carries a different text than the number of this sale, so it was not shown.",
            ),
            Self::WrongNode => (
                "Invoice from another node",
                "The invoice is signed by a node that is not the shop's Lightning node, so it was not shown.",
            ),
        };
        let next = if matches!(self, Self::WrongNode) {
            "Take another kind of payment. Tell the owner: the counter may be connected to the wrong node."
        } else {
            REPORT
        };
        sale(heading, what, next, self.rule(), self.to_string())
    }
}

impl AskRefused {
    /// What the cashier is told when an invoice is not asked for.
    pub fn words(&self) -> Words {
        let (heading, what) = match self {
            Self::BadAmount => (
                "Lightning amount not valid",
                "The amount of the sale is nothing, or more than all the bitcoin there is, so Lightning was not offered.",
            ),
            Self::BadExpiry => (
                "Lightning time not valid",
                "The time the price lasts is nothing, or longer than a day, so Lightning was not offered.",
            ),
        };
        sale(
            heading,
            what,
            "Take another kind of payment. Stop and report it to the OpenBMT project with the details.",
            self.rule(),
            self.to_string(),
        )
    }
}

impl UriRefused {
    /// What the cashier is told when the payment code is not made.
    pub fn words(&self) -> Words {
        let (heading, what) = match self {
            Self::BadAddress => (
                "Code address not valid",
                "The address for the code is not an address of this shop's bitcoin network, so no code was made.",
            ),
            Self::AmountMismatch => (
                "Code amount not matching",
                "The amount of the code is not the amount of the Lightning invoice, so no code was made.",
            ),
            Self::NotBech32 => (
                "Code cannot be in capitals",
                "The address is not of the kind that may be written in capitals, so the code stays as it is.",
            ),
        };
        sale(
            heading,
            what,
            "Take another kind of payment. Stop and report it to the OpenBMT project with the details.",
            self.rule(),
            self.to_string(),
        )
    }
}

impl ConnectionRefused {
    /// What the owner is told when the test of a connection fails.
    pub fn words(&self) -> Words {
        let lnd = self.rule() == "LN-06";
        let (heading, what, next) = match self {
            Self::CredentialTooStrong { .. } if lnd => (
                "Macaroon can do too much",
                "The node let the counter's macaroon do something other than invoices, so it can do more than it should.",
                NEW_MACAROON,
            ),
            Self::CredentialTooStrong { .. } => (
                "Rune can do too much",
                "The node let the counter's rune do something other than invoices, so it can do more than it should.",
                NEW_RUNE,
            ),
            Self::ProbeUnclear { .. } => (
                "Key limit not proven",
                "The node refused a test call, but not in the way that shows the key is limited to invoices.",
                "Update the node to its current release, then test again. If it repeats, make the key again.",
            ),
            Self::RuneNotFromNode => (
                "Rune not from this node",
                "This rune is not from this node: the node says it is not one it made, so it cannot be used with it.",
                NEW_RUNE,
            ),
            Self::RuneRevoked => (
                "Rune revoked on the node",
                "This rune has been revoked on the node, so it no longer works.",
                NEW_RUNE,
            ),
            Self::NoInvoice(_) => (
                "Test invoice not made",
                "The node did not make the test invoice that shows the connection works.",
                "Check that the node is running and unlocked and that the key may make invoices, then test again.",
            ),
            Self::LabelUsed => (
                "Test name already used",
                "The node already has an invoice under the name of this test.",
                "Run the test again, which uses a new name.",
            ),
            Self::WrongNode => (
                "Wrong Lightning node",
                "The test invoice is signed by a different node than the one in the shop's settings.",
                "Check the node's id on its own screen against the one the key holder approved.",
            ),
            Self::BadInvoice(_) => (
                "Test invoice not accepted",
                "The node made a test invoice that does not pass the counter's checks.",
                "Update the node to its current release and test again. If it repeats, report it to the OpenBMT project with the details.",
            ),
        };
        setup(heading, what, next, self.rule(), self.to_string())
    }
}

impl PaidFacts {
    /// What the cashier is told when the node says the payment is settled.
    pub fn words(&self) -> Words {
        let words = Words::new(
            Tone::Good,
            "Lightning payment in",
            "The shop's node says the Lightning payment is settled in full, and it cannot be undone.",
            Who::Cashier,
            "Nothing more to wait for. Hand over the goods.",
        )
        .code("LN-21")
        .detail(format!("received {} msat", self.amount_received_msat));
        match self.settled_at {
            Some(at) => words.at(at),
            None => words,
        }
    }
}

impl NotPaid {
    /// What the cashier is told while the payment is not paid.
    pub fn words(&self) -> Words {
        let w = |tone, heading: &str, what: &str, who, next: &str| {
            Words::new(tone, heading, what, who, next).code("LN-14")
        };
        match self {
            Self::Waiting => w(
                Tone::Info,
                "Waiting for Lightning",
                "The node has the invoice and nothing has been paid yet.",
                Who::Customer,
                "The customer scans the code and pays with their Lightning wallet.",
            ),
            Self::Expired => w(
                Tone::Ended,
                "Lightning invoice ran out",
                "The invoice ran out of time before it was paid, so nothing was received.",
                Who::Cashier,
                "Start the sale again for a new price, or take another kind of payment.",
            ),
            Self::Cancelled => w(
                Tone::Ended,
                "Lightning invoice cancelled",
                "The node cancelled the invoice, so nothing was received.",
                Who::Cashier,
                "Start the sale again, or take another kind of payment.",
            ),
            Self::Held => w(
                Tone::Info,
                "Lightning payment on hold",
                "A payment has reached the node, but the node has not settled it, so it is not paid yet.",
                Who::Nobody,
                "Wait. Do not hand over the goods until the node says it is settled.",
            ),
            Self::TooLittle { asked, received } => Words::new(
                Tone::Stop,
                "Lightning payment too small",
                "The node says the payment is settled, but for less than the sale.",
                Who::Cashier,
                "Do not hand over the goods. Ask for the rest another way, or refund what came.",
            )
            .code("LN-14")
            .detail(format!("asked {asked} msat, received {received} msat")),
            Self::NotFound => w(
                Tone::Stop,
                "Node has no such invoice",
                "The node has no invoice for this payment, so nothing can be said to be paid.",
                Who::Cashier,
                "Do not hand over the goods. Take another kind of payment and tell the owner.",
            ),
            Self::Unknown(state) => w(
                Tone::Stop,
                "Node state not known",
                "The node gave the invoice a state the counter does not know, so it is not counted as paid.",
                Who::Cashier,
                "Do not hand over the goods. Take another kind of payment and tell the owner.",
            )
            .quoted(Quoted::new("The node says the state is", state)),
        }
    }
}

/// The name of a credential refusal in the list.
pub fn credential_name(e: &CredentialRefused) -> &'static str {
    match e {
        CredentialRefused::BadAddress => "BadAddress",
        CredentialRefused::BadConnectString => "BadConnectString",
        CredentialRefused::NoCertificate => "NoCertificate",
        CredentialRefused::BadCertificate => "BadCertificate",
        CredentialRefused::BadFingerprint => "BadFingerprint",
        CredentialRefused::BadMacaroon => "BadMacaroon",
        CredentialRefused::RuneNotBase64 => "RuneNotBase64",
        CredentialRefused::RuneUnrestricted => "RuneUnrestricted",
        CredentialRefused::RuneNotText => "RuneNotText",
        CredentialRefused::RuneSyntax(_) => "RuneSyntax",
        CredentialRefused::RuneMixesMethod => "RuneMixesMethod",
        CredentialRefused::RuneAllowsMethod(_) => "RuneAllowsMethod",
        CredentialRefused::RuneLacksMethod => "RuneLacksMethod",
    }
}

/// The name of a reply refusal in the list.
pub fn reply_name(e: &ReplyRefused) -> &'static str {
    match e {
        ReplyRefused::NodeSaid { .. } => "NodeSaid",
        ReplyRefused::TooLarge => "TooLarge",
        ReplyRefused::Unreadable => "Unreadable",
        ReplyRefused::Missing(_) => "Missing",
        ReplyRefused::WrongType(_) => "WrongType",
        ReplyRefused::OutOfRange(_) => "OutOfRange",
        ReplyRefused::OtherInvoice => "OtherInvoice",
        ReplyRefused::Several => "Several",
        ReplyRefused::LabelTaken => "LabelTaken",
        ReplyRefused::LabelGone => "LabelGone",
    }
}

/// The name of an invoice refusal in the list.
pub fn invoice_name(e: &InvoiceRefused) -> &'static str {
    match e {
        InvoiceRefused::TooLong => "TooLong",
        InvoiceRefused::MixedCase => "MixedCase",
        InvoiceRefused::NotAnInvoice => "NotAnInvoice",
        InvoiceRefused::UnknownNetwork => "UnknownNetwork",
        InvoiceRefused::BadAmount => "BadAmount",
        InvoiceRefused::Truncated => "Truncated",
        InvoiceRefused::BadField(_) => "BadField",
        InvoiceRefused::DuplicateField(_) => "DuplicateField",
        InvoiceRefused::MissingField(_) => "MissingField",
        InvoiceRefused::BadSignature => "BadSignature",
        InvoiceRefused::WrongNetwork => "WrongNetwork",
        InvoiceRefused::NoAmount => "NoAmount",
        InvoiceRefused::WrongAmount { .. } => "WrongAmount",
        InvoiceRefused::WrongPaymentHash => "WrongPaymentHash",
        InvoiceRefused::WrongExpiry { .. } => "WrongExpiry",
        InvoiceRefused::WrongDescription => "WrongDescription",
        InvoiceRefused::WrongNode => "WrongNode",
    }
}

/// The name of a connection refusal in the list.
pub fn connection_name(e: &ConnectionRefused) -> &'static str {
    match e {
        ConnectionRefused::CredentialTooStrong { rule, .. } if *rule == "LN-06" => {
            "CredentialTooStrong (LND)"
        }
        ConnectionRefused::CredentialTooStrong { .. } => "CredentialTooStrong (Core Lightning)",
        ConnectionRefused::ProbeUnclear { .. } => "ProbeUnclear",
        ConnectionRefused::RuneNotFromNode => "RuneNotFromNode",
        ConnectionRefused::RuneRevoked => "RuneRevoked",
        ConnectionRefused::NoInvoice(_) => "NoInvoice",
        ConnectionRefused::LabelUsed => "LabelUsed",
        ConnectionRefused::WrongNode => "WrongNode",
        ConnectionRefused::BadInvoice(_) => "BadInvoice",
    }
}

/// The name of a state of an unpaid invoice in the list.
pub fn not_paid_name(e: &NotPaid) -> &'static str {
    match e {
        NotPaid::Waiting => "Waiting",
        NotPaid::Expired => "Expired",
        NotPaid::Cancelled => "Cancelled",
        NotPaid::Held => "Held",
        NotPaid::TooLittle { .. } => "TooLittle",
        NotPaid::NotFound => "NotFound",
        NotPaid::Unknown(_) => "Unknown",
    }
}

/// Every credential refusal, with a value for each variant.
pub fn all_credential() -> Vec<CredentialRefused> {
    vec![
        CredentialRefused::BadAddress,
        CredentialRefused::BadConnectString,
        CredentialRefused::NoCertificate,
        CredentialRefused::BadCertificate,
        CredentialRefused::BadFingerprint,
        CredentialRefused::BadMacaroon,
        CredentialRefused::RuneNotBase64,
        CredentialRefused::RuneUnrestricted,
        CredentialRefused::RuneNotText,
        CredentialRefused::RuneSyntax("an operator"),
        CredentialRefused::RuneMixesMethod,
        CredentialRefused::RuneAllowsMethod("pay".to_string()),
        CredentialRefused::RuneLacksMethod,
    ]
}

/// Every reply refusal.
pub fn all_reply() -> Vec<ReplyRefused> {
    vec![
        ReplyRefused::NodeSaid {
            status: 500,
            message: "unable to locate invoice".to_string(),
        },
        ReplyRefused::TooLarge,
        ReplyRefused::Unreadable,
        ReplyRefused::Missing("payment_request"),
        ReplyRefused::WrongType("state"),
        ReplyRefused::OutOfRange("amt_paid_msat"),
        ReplyRefused::OtherInvoice,
        ReplyRefused::Several,
        ReplyRefused::LabelTaken,
        ReplyRefused::LabelGone,
    ]
}

/// Every invoice refusal.
pub fn all_invoice() -> Vec<InvoiceRefused> {
    vec![
        InvoiceRefused::TooLong,
        InvoiceRefused::MixedCase,
        InvoiceRefused::NotAnInvoice,
        InvoiceRefused::UnknownNetwork,
        InvoiceRefused::BadAmount,
        InvoiceRefused::Truncated,
        InvoiceRefused::BadField("payment hash"),
        InvoiceRefused::DuplicateField("expiry"),
        InvoiceRefused::MissingField("payment hash"),
        InvoiceRefused::BadSignature,
        InvoiceRefused::WrongNetwork,
        InvoiceRefused::NoAmount,
        InvoiceRefused::WrongAmount {
            asked: 1_000,
            got: 2_000,
        },
        InvoiceRefused::WrongPaymentHash,
        InvoiceRefused::WrongExpiry {
            asked: 300,
            got: 3_600,
        },
        InvoiceRefused::WrongDescription,
        InvoiceRefused::WrongNode,
    ]
}

/// Every refusal to ask for an invoice.
pub fn all_ask() -> Vec<AskRefused> {
    vec![AskRefused::BadAmount, AskRefused::BadExpiry]
}

/// Every refusal to make the code.
pub fn all_uri() -> Vec<UriRefused> {
    vec![
        UriRefused::BadAddress,
        UriRefused::AmountMismatch,
        UriRefused::NotBech32,
    ]
}

/// Every failure of a connection test, with a value for each variant and for
/// each kind of node where the words differ.
pub fn all_connection() -> Vec<ConnectionRefused> {
    vec![
        ConnectionRefused::CredentialTooStrong {
            call: "GET /v1/getinfo",
            rule: "LN-06",
        },
        ConnectionRefused::CredentialTooStrong {
            call: "POST /v1/listfunds",
            rule: "LN-09",
        },
        ConnectionRefused::ProbeUnclear {
            call: "GET /v1/getinfo",
            status: 404,
            rule: "LN-06",
        },
        ConnectionRefused::RuneNotFromNode,
        ConnectionRefused::RuneRevoked,
        ConnectionRefused::NoInvoice(ReplyRefused::TooLarge),
        ConnectionRefused::LabelUsed,
        ConnectionRefused::WrongNode,
        ConnectionRefused::BadInvoice(InvoiceRefused::WrongAmount {
            asked: 1_000,
            got: 2_000,
        }),
    ]
}

/// Every state of an invoice that is not paid.
pub fn all_not_paid() -> Vec<NotPaid> {
    vec![
        NotPaid::Waiting,
        NotPaid::Expired,
        NotPaid::Cancelled,
        NotPaid::Held,
        NotPaid::TooLittle {
            asked: 5_000,
            received: 4_000,
        },
        NotPaid::NotFound,
        NotPaid::Unknown("HOLD".to_string()),
    ]
}

/// SC-12: the words of every state and refusal this crate can give, at least
/// one value for each variant.
pub fn all_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    for e in all_credential() {
        all.push((
            format!("CredentialRefused::{}", credential_name(&e)),
            e.words(),
        ));
    }
    for e in all_reply() {
        all.push((format!("ReplyRefused::{}", reply_name(&e)), e.words()));
    }
    for e in all_invoice() {
        all.push((format!("InvoiceRefused::{}", invoice_name(&e)), e.words()));
    }
    for e in all_ask() {
        all.push((format!("AskRefused::{e:?}"), e.words()));
    }
    for e in all_uri() {
        all.push((format!("UriRefused::{e:?}"), e.words()));
    }
    for e in all_connection() {
        all.push((
            format!("ConnectionRefused::{}", connection_name(&e)),
            e.words(),
        ));
    }
    let paid = PaidFacts {
        payment_hash: [7; 32],
        amount_received_msat: 5_000,
        settled_at: Some(1_700_000_000),
    };
    all.push(("Payment::Paid".to_string(), paid.words()));
    for e in all_not_paid() {
        all.push((format!("NotPaid::{}", not_paid_name(&e)), e.words()));
    }
    all
}

/// SC-12: the list of words as the table of `docs/screens/lightning.md`.
pub fn screens_markdown() -> String {
    let who = |w: Who| match w {
        Who::Nobody => "nobody",
        Who::Customer => "the customer",
        Who::Cashier => "the cashier",
        Who::Manager => "a manager",
        Who::Owner => "the owner",
        Who::KeyHolder => "the key holder",
        Who::BoxKeeper => "the Box keeper",
    };
    let cell = |text: &str| text.replace('|', "/");
    let mut out = String::from(
        "# Lightning: what a person is told\n\n\
         Made by `screens_markdown` in `crates/openbmt-lightning/src/say.rs`. Do\n\
         not edit it by hand: run the test `ln_22_the_list_is_on_disk` with the\n\
         environment variable `OPENBMT_WRITE_SCREENS=1` to write it again.\n\n\
         | State | Heading | What happened | What to do next | Who | Code |\n\
         |---|---|---|---|---|---|\n",
    );
    for (name, words) in all_words() {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            cell(&name),
            cell(&words.heading),
            cell(&words.what),
            cell(&words.next),
            who(words.who),
            words.code.unwrap_or("")
        ));
    }
    out
}
