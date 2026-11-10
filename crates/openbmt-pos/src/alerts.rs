//! The alerts (PS-40 to PS-52). Each says why it was raised and what to do
//! next, in plain words.

use openbmt_counter::Currency;

use crate::facts::SaleKey;
use crate::model::{OrderId, TenderId};

/// What an alert is about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Subject {
    /// A tender of an order.
    Tender(TenderKey),
    /// A counter sale.
    Sale(SaleKey),
}

/// Which tender: the order and the tender id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TenderKey {
    /// The order.
    pub order: OrderId,
    /// The tender.
    pub tender: TenderId,
}

/// The kinds of alert (PS-40 to PS-48).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AlertKind {
    /// PS-40: a Bitcoin tender with no sale.
    TenderWithoutPayment,
    /// PS-41: the order was closed before the payment was safe.
    ClosedBeforePaid,
    /// PS-42: a paid sale with no tender.
    PaymentWithoutTender,
    /// PS-43: a reference named a sale whose amount differs.
    AmountDiffers,
    /// PS-44: the matched sale is not paid.
    TenderOnUnpaidSale,
    /// PS-45: the order or tender was voided or refunded after payment.
    VoidedAfterPayment,
    /// PS-46: one sale named by two tenders.
    DuplicateReference,
    /// PS-47: a tip on a Bitcoin tender.
    BitcoinTip,
    /// PS-48: a currency the shop does not use.
    UnknownCurrency,
}

impl AlertKind {
    /// The byte the kind is stored as.
    pub(crate) fn to_u8(self) -> u8 {
        match self {
            Self::TenderWithoutPayment => 1,
            Self::ClosedBeforePaid => 2,
            Self::PaymentWithoutTender => 3,
            Self::AmountDiffers => 4,
            Self::TenderOnUnpaidSale => 5,
            Self::VoidedAfterPayment => 6,
            Self::DuplicateReference => 7,
            Self::BitcoinTip => 8,
            Self::UnknownCurrency => 9,
        }
    }

    /// Read a kind byte.
    pub(crate) fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::TenderWithoutPayment,
            2 => Self::ClosedBeforePaid,
            3 => Self::PaymentWithoutTender,
            4 => Self::AmountDiffers,
            5 => Self::TenderOnUnpaidSale,
            6 => Self::VoidedAfterPayment,
            7 => Self::DuplicateReference,
            8 => Self::BitcoinTip,
            9 => Self::UnknownCurrency,
            _ => return None,
        })
    }

    /// A short name for a list.
    pub fn name(&self) -> &'static str {
        match self {
            Self::TenderWithoutPayment => "Bitcoin tender with no payment",
            Self::ClosedBeforePaid => "Order closed before the payment was safe",
            Self::PaymentWithoutTender => "Payment with no order",
            Self::AmountDiffers => "Amount differs",
            Self::TenderOnUnpaidSale => "Bitcoin tender on a sale that is not paid",
            Self::VoidedAfterPayment => "Order voided after payment",
            Self::DuplicateReference => "Same reference used twice",
            Self::BitcoinTip => "Tip on a Bitcoin tender",
            Self::UnknownCurrency => "Bitcoin tender in another currency",
        }
    }
}

/// An alert's identity (PS-50): its kind and its subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AlertKey {
    /// The kind.
    pub kind: AlertKind,
    /// What it is about.
    pub subject: Subject,
}

/// An alert, with what its texts need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    /// Its identity.
    pub key: AlertKey,
    /// The sale it was worked out against, if any. When that sale is no
    /// longer among the facts, the alert cannot be worked out again (PS-51).
    pub sale: Option<SaleKey>,
    /// The currency the amounts are in, if the alert has amounts.
    pub currency: Option<Currency>,
    /// The tender's amount in minor units (its tip, for a tip alert).
    pub tender_minor: u64,
    /// The sale's amount in minor units, when there is a sale.
    pub sale_minor: u64,
    /// The time of the thing at issue: the tender's, or when the sale was
    /// first reported paid.
    pub at: u64,
    /// A span in seconds the text names (how long before the paid time).
    pub secs: u64,
    /// When the alert was raised.
    pub raised_at: u64,
}

fn money(minor: u64, currency: Option<Currency>) -> String {
    let code = currency.map_or_else(String::new, |c| format!(" {c}"));
    format!("{}.{:02}{code}", minor / 100, minor % 100)
}

impl Alert {
    /// PS-49: why the alert was raised, in plain words.
    pub fn why(&self) -> String {
        let tender = money(self.tender_minor, self.currency);
        let sale = money(self.sale_minor, self.currency);
        match self.key.kind {
            AlertKind::TenderWithoutPayment => format!(
                "The POS has a Bitcoin tender of {tender} and no payment at the \
                 counter lines up with it."
            ),
            AlertKind::ClosedBeforePaid => format!(
                "The POS order was closed with a Bitcoin tender {} minutes before the \
                 counter showed the payment as paid.",
                self.secs / 60
            ),
            AlertKind::PaymentWithoutTender => format!(
                "The counter showed a sale of {sale} as paid and no Bitcoin tender \
                 in the POS lines up with it."
            ),
            AlertKind::AmountDiffers => {
                format!("A Bitcoin tender of {tender} names a counter sale that was for {sale}.")
            }
            AlertKind::TenderOnUnpaidSale => format!(
                "A Bitcoin tender of {tender} lines up with a counter sale that is not \
                 paid: it was either never paid or was reported unpaid again."
            ),
            AlertKind::VoidedAfterPayment => format!(
                "The POS order or its Bitcoin tender of {tender} was voided or refunded, \
                 and the counter sale of {sale} is still paid."
            ),
            AlertKind::DuplicateReference => format!(
                "A Bitcoin tender of {tender} names a counter sale that another tender \
                 already holds."
            ),
            AlertKind::BitcoinTip => format!(
                "A Bitcoin tender carries a tip of {tender}. The counter takes no tip, \
                 so the tip was not paid in bitcoin."
            ),
            AlertKind::UnknownCurrency => {
                format!("A Bitcoin tender of {tender} is in a currency the shop has not set up.")
            }
        }
    }

    /// PS-49: what to do next, in plain words.
    pub fn next(&self) -> &'static str {
        match self.key.kind {
            AlertKind::TenderWithoutPayment => {
                "Ask the cashier. If the customer paid, check the counter's sale list \
                 and the wallet. If not, the order was closed without payment. If the \
                 order's location is not set up here, fix the settings."
            }
            AlertKind::ClosedBeforePaid => {
                "Check that the goods were handed over only after the counter showed \
                 green. The order may have been closed too early."
            }
            AlertKind::PaymentWithoutTender => {
                "Find the order in the POS and close it with the Bitcoin tender, or \
                 refund the customer. If this counter is used without a POS, mark it \
                 so in the settings."
            }
            AlertKind::AmountDiffers => {
                "Compare the order and the counter's sale. The customer may be owed \
                 the difference, or the cashier typed the wrong reference."
            }
            AlertKind::TenderOnUnpaidSale => {
                "Do not hand over the goods. Look at the sale on the counter: if the \
                 payment was replaced, the customer must pay again."
            }
            AlertKind::VoidedAfterPayment => {
                "Either the customer is owed a refund, or the cashier undid the \
                 order. Ask the cashier and check the refund queue."
            }
            AlertKind::DuplicateReference => {
                "Two orders were closed for one payment. Ask the cashier which order \
                 the payment was for and void the other."
            }
            AlertKind::BitcoinTip => {
                "Find out how the tip was paid. It did not come through the counter."
            }
            AlertKind::UnknownCurrency => {
                "Add the currency to the settings if the shop uses it, or ask the \
                 cashier why the order is in it."
            }
        }
    }
}
