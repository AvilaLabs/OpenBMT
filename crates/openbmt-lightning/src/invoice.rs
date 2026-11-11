//! What is asked of the node and what it may answer: the invoice of one sale
//! (LN-13) and whether it is paid (LN-14, LN-21).

use crate::error::AskRefused;
use crate::text::hex;

/// Millisatoshis in one satoshi.
pub const MSAT_PER_SAT: u64 = 1_000;
/// All the bitcoin there is, in millisatoshis.
pub const MAX_MSAT: u64 = 2_100_000_000_000_000 * MSAT_PER_SAT;
/// The longest lifetime of an invoice the crate asks for, in seconds.
pub const MAX_EXPIRY_SECS: u32 = 86_400;
/// The amount of the test invoice of a connection test: one satoshi.
pub const TEST_AMOUNT_MSAT: u64 = MSAT_PER_SAT;
/// The lifetime of the test invoice, in seconds.
pub const TEST_EXPIRY_SECS: u32 = 300;

/// The description of the invoice of a sale (LN-13): only the number of the
/// sale. The payer's wallet shows it, so it holds no shop name and no item.
pub fn sale_description(sale_number: u64) -> String {
    format!("Sale {sale_number}")
}

/// The description of the test invoice of a connection test.
pub const TEST_DESCRIPTION: &str = "OpenBMT test";

/// One invoice the counter asks the node for (LN-13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    amount_msat: u64,
    expiry_secs: u32,
    description: String,
    label: String,
}

impl Ask {
    /// The invoice of a sale: `amount_msat` (the sale's satoshis times
    /// 1,000), as long as the counter's quote lasts, and for Core Lightning
    /// the label `obmt-<counter>-<sale>-<attempt>`, which is the same each
    /// time the same attempt is made (LN-15). `counter` is the first four
    /// bytes of the counter's device id. A retry after a timeout uses the
    /// same attempt; a retry after the node said the label belongs to an
    /// invoice that is not this sale's uses the next one.
    ///
    /// `expiry_secs` is the quote's lifetime (`openbmt_counter::money::
    /// QUOTE_SECS`, 300, CR-05). It is a parameter because it lives in the
    /// counter crate, which this one does not depend on.
    pub fn sale(
        counter: [u8; 4],
        sale_number: u64,
        attempt: u32,
        amount_msat: u64,
        expiry_secs: u32,
    ) -> Result<Self, AskRefused> {
        Self::checked(
            amount_msat,
            expiry_secs,
            sale_description(sale_number),
            format!("obmt-{}-{sale_number}-{attempt}", hex(&counter)),
        )
    }

    /// The test invoice of a connection test: one satoshi, which expires by
    /// itself, with a label of its own.
    pub fn test(counter: [u8; 4], test_id: u64) -> Self {
        Self {
            amount_msat: TEST_AMOUNT_MSAT,
            expiry_secs: TEST_EXPIRY_SECS,
            description: TEST_DESCRIPTION.to_string(),
            label: format!("obmt-{}-test-{test_id}", hex(&counter)),
        }
    }

    fn checked(
        amount_msat: u64,
        expiry_secs: u32,
        description: String,
        label: String,
    ) -> Result<Self, AskRefused> {
        if amount_msat == 0 || amount_msat > MAX_MSAT {
            return Err(AskRefused::BadAmount);
        }
        if expiry_secs == 0 || expiry_secs > MAX_EXPIRY_SECS {
            return Err(AskRefused::BadExpiry);
        }
        Ok(Self {
            amount_msat,
            expiry_secs,
            description,
            label,
        })
    }

    /// The amount, in millisatoshis.
    pub fn amount_msat(&self) -> u64 {
        self.amount_msat
    }

    /// The lifetime of the invoice, in seconds.
    pub fn expiry_secs(&self) -> u32 {
        self.expiry_secs
    }

    /// The description the invoice must carry.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The label of the invoice at Core Lightning.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// An invoice the node made, as the node reported it. It is not trusted until
/// `bolt11::check` has passed it (LN-17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// The payment hash the node reported.
    pub payment_hash: [u8; 32],
    /// The BOLT 11 invoice the node returned.
    pub bolt11: String,
}

/// What the node did with a request for an invoice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    /// It made the invoice.
    Created(Created),
    /// Core Lightning refused the label as used already (error 900): the
    /// invoice may be the one an earlier try made and whose reply was lost.
    /// Ask for it by label (LN-15).
    LabelExists,
}

/// The facts of a payment the node says is settled (LN-21). Lightning payments
/// are final at once (DESIGN section 8), so these are all the counter keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaidFacts {
    /// The payment hash of the invoice.
    pub payment_hash: [u8; 32],
    /// What the node received, in millisatoshis: at least what was asked.
    pub amount_received_msat: u64,
    /// When the node says it settled, in seconds since 1970, if it says.
    pub settled_at: Option<u64>,
}

/// Why a payment is not paid (LN-14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotPaid {
    /// The invoice is open and nothing has arrived.
    Waiting,
    /// The invoice ran out of time.
    Expired,
    /// The node cancelled the invoice.
    Cancelled,
    /// LND holds a payment for the invoice that it has not settled.
    Held,
    /// The node says it is paid, for less than was asked.
    TooLittle {
        /// Millisatoshis asked.
        asked: u64,
        /// Millisatoshis received.
        received: u64,
    },
    /// The node has no such invoice.
    NotFound,
    /// A state the crate does not know, as the node wrote it (cleaned).
    Unknown(String),
}

/// Whether the node says an invoice is paid (LN-14). An error reading the
/// reply is not a payment either: only [`Payment::Paid`] is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payment {
    /// The node says it is settled, for at least the amount asked.
    Paid(PaidFacts),
    /// It is not.
    NotPaid(NotPaid),
}

/// The state a node reports as paid, judged against the amount asked.
pub(crate) fn judge_paid(
    payment_hash: [u8; 32],
    received: u64,
    settled_at: Option<u64>,
    asked: u64,
) -> Payment {
    if received >= asked {
        Payment::Paid(PaidFacts {
            payment_hash,
            amount_received_msat: received,
            settled_at,
        })
    } else {
        Payment::NotPaid(NotPaid::TooLittle { asked, received })
    }
}
