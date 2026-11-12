//! What the owner is told about requests (RQ-35, RQ-36, SC-01 to SC-12).
//!
//! The texts are the ones of `words-owner-requests.md`, as they are
//! written there. Where a text breaks the screen rules (a heading over 28
//! characters, a button where a sentence is wanted) or is not written, the
//! function gives the constant that names its rule, `WORDS_NEEDED_RQ_xx`,
//! and nothing else: no text is made up here. Amounts and dates are given
//! as text by the caller, in the shop's currency where it is known (SC-09).

use openbmt_core::say::{sats, Tone, Who, Words};

use crate::error::{
    AddressRefused, BookError, CreditRefused, EndRefused, Limit, MakeRefused, RepeatRefused,
    ReturnRefused, ShareRefused,
};
use crate::model::{Marks, Status};

/// Words are not written for a refusal that has none: the rule is named.
pub type Said = Result<Words, &'static str>;

pub const WORDS_NEEDED_RQ_03_ADDRESS_USED: &str = "WORDS_NEEDED_RQ_03_ADDRESS_USED";
pub const WORDS_NEEDED_RQ_07_BITCOIN_ONLY: &str = "WORDS_NEEDED_RQ_07_BITCOIN_ONLY";
pub const WORDS_NEEDED_RQ_09_DUE_OUT_OF_RANGE: &str = "WORDS_NEEDED_RQ_09_DUE_OUT_OF_RANGE";
pub const WORDS_NEEDED_RQ_12_NOT_OPEN: &str = "WORDS_NEEDED_RQ_12_NOT_OPEN";
pub const WORDS_NEEDED_RQ_15_NO_RATE_TO_CREDIT: &str = "WORDS_NEEDED_RQ_15_NO_RATE_TO_CREDIT";
pub const WORDS_NEEDED_RQ_15_NOT_IN_BLOCK_ORDER: &str = "WORDS_NEEDED_RQ_15_NOT_IN_BLOCK_ORDER";
pub const WORDS_NEEDED_RQ_15_TOO_MANY_PAYMENTS: &str = "WORDS_NEEDED_RQ_15_TOO_MANY_PAYMENTS";
pub const WORDS_NEEDED_RQ_16_NOT_ALMOST: &str = "WORDS_NEEDED_RQ_16_NOT_ALMOST";
pub const WORDS_NEEDED_RQ_19_HAS_PAYMENTS: &str = "WORDS_NEEDED_RQ_19_HAS_PAYMENTS";
pub const WORDS_NEEDED_RQ_19_ALREADY_ENDED: &str = "WORDS_NEEDED_RQ_19_ALREADY_ENDED";
pub const WORDS_NEEDED_RQ_20_NOT_PART_PAID: &str = "WORDS_NEEDED_RQ_20_NOT_PART_PAID";
pub const WORDS_NEEDED_RQ_20_CLOSE_CONFIRM: &str = "WORDS_NEEDED_RQ_20_CLOSE_CONFIRM";
pub const WORDS_NEEDED_RQ_21_NO_SUCH_REQUEST: &str = "WORDS_NEEDED_RQ_21_NO_SUCH_REQUEST";
pub const WORDS_NEEDED_RQ_25_NO_REPEAT: &str = "WORDS_NEEDED_RQ_25_NO_REPEAT";
pub const WORDS_NEEDED_RQ_25_REPEAT_NOT_DUE: &str = "WORDS_NEEDED_RQ_25_REPEAT_NOT_DUE";
pub const WORDS_NEEDED_RQ_26_TOO_SMALL: &str = "WORDS_NEEDED_RQ_26_TOO_SMALL";
pub const WORDS_NEEDED_RQ_27_ASK_FOR_AN_ADDRESS: &str = "WORDS_NEEDED_RQ_27_ASK_FOR_AN_ADDRESS";
pub const WORDS_NEEDED_RQ_28_CHECK_THE_ADDRESS: &str = "WORDS_NEEDED_RQ_28_CHECK_THE_ADDRESS";
pub const WORDS_NEEDED_RQ_29_QUEUE_FULL: &str = "WORDS_NEEDED_RQ_29_QUEUE_FULL";
pub const WORDS_NEEDED_RQ_29_RETURNS_WAITING: &str = "WORDS_NEEDED_RQ_29_RETURNS_WAITING";
pub const WORDS_NEEDED_RQ_30_BOOK_FULL: &str = "WORDS_NEEDED_RQ_30_BOOK_FULL";

fn refused(heading: &str, what: impl Into<String>, next: &str, code: &'static str) -> Words {
    Words::new(Tone::Ended, heading, what, Who::Owner, next).code(code)
}

/// RQ-08: "Amount too small".
fn too_small(min_sats: u64) -> Words {
    refused(
        "Amount too small",
        format!(
            "Requests must be at least {}, the smallest bitcoin sale in your settings.",
            sats(min_sats)
        ),
        "Ask for more, or change the setting.",
        "RQ-08",
    )
}

/// RQ-08: "Amount too large".
fn too_large(max: &Limit) -> Words {
    let max = match max {
        Limit::Sats(n) => sats(*n),
        Limit::Units(currency, n) => currency.whole_text(*n),
    };
    refused(
        "Amount too large",
        format!("A request can be at most {max}."),
        "Split it into more than one request.",
        "RQ-08",
    )
}

/// RQ-12: the words of a stale rate at the counter, as they are there.
pub fn stale_rate() -> Words {
    refused(
        "Price too old to renew",
        "The exchange price is too old to give this sale a new price.",
        "Wait for a new price, or check the internet connection. If it stays old, tell the owner.",
        "CR-58",
    )
}

/// Words for a request that was not made.
pub fn make_refused(why: &MakeRefused) -> Said {
    match why {
        MakeRefused::NoteNeeded => Ok(refused(
            "Note needed",
            "Give the request a name of 1 to 64 characters so you can find it later.",
            "Type a short note.",
            "RQ-06",
        )),
        MakeRefused::BitcoinOnly => Err(WORDS_NEEDED_RQ_07_BITCOIN_ONLY),
        MakeRefused::DueOutOfRange => Err(WORDS_NEEDED_RQ_09_DUE_OUT_OF_RANGE),
        MakeRefused::TooManyOpen => Ok(too_many_open()),
        MakeRefused::TooSmall { min_sats } => Ok(too_small(*min_sats)),
        MakeRefused::TooLarge { max } => Ok(too_large(max)),
        MakeRefused::StaleRate => Ok(stale_rate()),
        MakeRefused::IndexUsed => Err(WORDS_NEEDED_RQ_03_ADDRESS_USED),
        MakeRefused::BookFull => Err(WORDS_NEEDED_RQ_30_BOOK_FULL),
    }
}

fn too_many_open() -> Words {
    refused(
        "Too many open requests",
        "You can have up to 1,000 open requests.",
        "Close or cancel some, then try again.",
        "RQ-11",
    )
}

/// Words for a quote that was not made.
pub fn share_refused(why: &ShareRefused) -> Said {
    match why {
        ShareRefused::NoSuchRequest => Err(WORDS_NEEDED_RQ_21_NO_SUCH_REQUEST),
        ShareRefused::NotOpen => Err(WORDS_NEEDED_RQ_12_NOT_OPEN),
        ShareRefused::StaleRate => Ok(stale_rate()),
    }
}

/// Words for payments that were not counted.
pub fn credit_refused(why: &CreditRefused) -> Said {
    match why {
        CreditRefused::NoSuchRequest => Err(WORDS_NEEDED_RQ_21_NO_SUCH_REQUEST),
        CreditRefused::NotInBlockOrder => Err(WORDS_NEEDED_RQ_15_NOT_IN_BLOCK_ORDER),
        CreditRefused::NoRate => Err(WORDS_NEEDED_RQ_15_NO_RATE_TO_CREDIT),
        CreditRefused::TooManyPayments => Err(WORDS_NEEDED_RQ_15_TOO_MANY_PAYMENTS),
    }
}

/// Words for a request that was not cancelled, closed or counted as paid.
pub fn end_refused(why: &EndRefused) -> Said {
    match why {
        EndRefused::NoSuchRequest => Err(WORDS_NEEDED_RQ_21_NO_SUCH_REQUEST),
        EndRefused::AlreadyEnded => Err(WORDS_NEEDED_RQ_19_ALREADY_ENDED),
        EndRefused::HasPayments => Err(WORDS_NEEDED_RQ_19_HAS_PAYMENTS),
        EndRefused::NotPartPaid => Err(WORDS_NEEDED_RQ_20_NOT_PART_PAID),
        EndRefused::NotAlmost => Err(WORDS_NEEDED_RQ_16_NOT_ALMOST),
        EndRefused::NoRepeat => Err(WORDS_NEEDED_RQ_25_NO_REPEAT),
    }
}

/// Words for a repeat that was not made.
pub fn repeat_refused(why: &RepeatRefused) -> Said {
    match why {
        RepeatRefused::NotDue => Err(WORDS_NEEDED_RQ_25_REPEAT_NOT_DUE),
        RepeatRefused::TooManyOpen => Ok(too_many_open()),
        RepeatRefused::IndexUsed => Err(WORDS_NEEDED_RQ_03_ADDRESS_USED),
        RepeatRefused::BookFull => Err(WORDS_NEEDED_RQ_30_BOOK_FULL),
    }
}

/// Words for a return that was not queued. The amount of the "too much"
/// refusal is written in satoshis.
pub fn return_refused(why: &ReturnRefused) -> Said {
    match why {
        ReturnRefused::NoSuchRequest => Err(WORDS_NEEDED_RQ_21_NO_SUCH_REQUEST),
        ReturnRefused::TooMuch { most_sats } => Ok(refused(
            "More than was paid",
            format!(
                "You can return at most {} from this request.",
                sats(*most_sats)
            ),
            "Lower the amount.",
            "RQ-26",
        )),
        ReturnRefused::TooSmall { .. } => Err(WORDS_NEEDED_RQ_26_TOO_SMALL),
        ReturnRefused::StaleRate => Ok(stale_rate()),
        ReturnRefused::BadScript => Ok(address_refused(&AddressRefused::NotAnAddress)),
        ReturnRefused::QueueFull => Err(WORDS_NEEDED_RQ_29_QUEUE_FULL),
        ReturnRefused::BookFull => Err(WORDS_NEEDED_RQ_30_BOOK_FULL),
    }
}

/// RQ-28: words for an address that was refused.
pub fn address_refused(why: &AddressRefused) -> Words {
    match why {
        AddressRefused::NotAnAddress => refused(
            "Not an address",
            "This is not a bitcoin address OpenBMT can pay.",
            "Ask the customer to send it again.",
            "RQ-28",
        ),
        AddressRefused::TestNetwork => refused(
            "Test network address",
            "This address is for a test network, not real bitcoin.",
            "Ask the customer for a real bitcoin address.",
            "RQ-28",
        ),
        AddressRefused::Own => refused(
            "Your own address",
            "This address belongs to your shop's wallet.",
            "Ask the customer for an address of theirs.",
            "RQ-28",
        ),
    }
}

/// RQ-31: words for a book that cannot be read. The detail is for the small
/// print.
pub fn book_unreadable(why: &BookError) -> Words {
    let detail = match why {
        BookError::Io(e) => e.clone(),
        BookError::Corrupt(why) => (*why).to_string(),
    };
    Words::new(
        Tone::Stop,
        "Requests cannot be read",
        "The file that keeps your requests could not be read, so no new request can be made.",
        Who::Owner,
        "Do not clear the app's data. The file is kept as it is, and your money is in your wallet, not in this file.",
    )
    .code("RQ-31")
    .detail(detail)
}

/// RQ-18: a request in the currency credited at a quote that had run out.
pub fn old_price() -> Words {
    Words::new(
        Tone::Info,
        "On an old price",
        "This payment came after the price you sent had run out, so it was counted at that old price.",
        Who::Owner,
        "Leave it, or send again to ask for what is still owed at today's price.",
    )
    .code("RQ-18")
}

/// RQ-29: a return was queued.
pub fn return_added(amount: &str, number: u64) -> Words {
    Words::new(
        Tone::Good,
        "Return added",
        format!("{amount} to request No. {number} is waiting with your other returns."),
        Who::Owner,
        "Pay them together when you are ready.",
    )
    .code("RQ-29")
}

/// A dialog that asks "are you sure": a title, a text and two buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    /// The title.
    pub title: String,
    /// What happens, in one or two sentences.
    pub text: String,
    /// The button that does it.
    pub yes: &'static str,
    /// The button that does not.
    pub no: &'static str,
}

/// RQ-19: "Cancel request No. {n}?".
pub fn cancel_confirm(number: u64) -> Confirm {
    Confirm {
        title: format!("Cancel request No. {number}?"),
        text: "Its address keeps being watched. Anything paid to it later shows here, ready to send back.".to_string(),
        yes: "Cancel the request",
        no: "Keep it",
    }
}

/// RQ-16: "Count No. {n} as paid?".
pub fn count_as_paid_confirm(number: u64, short: &str) -> Confirm {
    Confirm {
        title: format!("Count No. {number} as paid?"),
        text: format!("It is {short} short. This cannot be undone."),
        yes: "Count as paid",
        no: "Keep it open",
    }
}

/// RQ-20: the words of "Close as it is" are written with a title over 28
/// characters; the rule is named until they are shortened.
pub fn close_confirm() -> Result<Confirm, &'static str> {
    Err(WORDS_NEEDED_RQ_20_CLOSE_CONFIRM)
}

/// RQ-13: the line above a shared link.
pub fn share_line(shop: &str, amount: &str, number: u64, due: &str) -> String {
    format!("{shop} asks for {amount}. Request No. {number}, due {due}.")
}

/// RQ-22: the line above a link that is sent again for an overdue request.
pub fn overdue_line(shop: &str, amount: &str, number: u64, due: &str) -> String {
    format!("{shop} still asks for {amount}. Request No. {number} was due {due}.")
}

/// RQ-27: the line that asks a customer for an address.
pub fn ask_line(shop: &str, amount: &str, number: u64) -> String {
    format!(
        "{shop} would like to return {amount} for request No. {number}. Please reply with a bitcoin address of yours."
    )
}

/// RQ-21: Home's row when some requests are overdue.
pub fn overdue_row(count: usize) -> String {
    format!("{count} overdue")
}

/// RQ-21: the second line of a row of the list.
pub fn list_row(number: u64, owed: &str, due: &str) -> String {
    format!("No. {number} \u{b7} {owed} still owed \u{b7} due {due}")
}

/// RQ-35: the big line of a request's page. `amount` writes an amount of
/// the request's unit as text.
pub fn state_line(status: &Status, amount: &dyn Fn(u64) -> String) -> String {
    match status {
        Status::Waiting => "Waiting for payment".to_string(),
        Status::PartPaid { owed } => format!("{} still owed", amount(*owed)),
        Status::Paid => "Paid".to_string(),
        Status::Overpaid { excess } => format!("Paid, with {} extra", amount(*excess)),
        Status::Almost { short } => format!("Almost paid: {} short", amount(*short)),
    }
}

/// RQ-35: the small lines under the state, one for each mark.
pub fn mark_lines(marks: &Marks) -> Vec<&'static str> {
    let mut lines = Vec::new();
    if marks.late {
        lines.push("Paid late");
    }
    if marks.old_price {
        lines.push("On an old price");
    }
    if marks.after_cancel {
        lines.push("Paid after it was cancelled");
    }
    if marks.after_close {
        lines.push("Paid after it was closed");
    }
    lines
}

/// RQ-35: "Due {date}" of a request's facts.
pub fn due_fact(due: &str) -> String {
    format!("Due {due}")
}

/// RQ-29: the line of a return that is done.
pub fn returned_line(amount: &str, date: &str) -> String {
    format!("Returned {amount} \u{b7} {date}")
}

/// RQ-35: the empty list: a heading and a text, and no button.
pub fn empty_list() -> (&'static str, &'static str) {
    (
        "No open requests",
        "Requests you make show here until they are paid.",
    )
}

#[cfg(test)]
#[path = "say_tests.rs"]
mod tests;
