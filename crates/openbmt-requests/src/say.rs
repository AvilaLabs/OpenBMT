//! What the owner is told about requests (RQ-35, RQ-36, SC-01 to SC-12).
//!
//! The texts are the ones of `words-owner-requests.md`, as they are
//! written there. Every refusal has words. Amounts and dates are given as
//! text by the caller, in the shop's currency where it is known (SC-09).

use openbmt_core::say::{sats, Tone, Who, Words};

use crate::error::{
    AddressRefused, BookError, CreditRefused, EndRefused, Limit, MakeRefused, RepeatRefused,
    ReturnRefused, ShareRefused,
};
use crate::model::{Marks, Status};

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

/// RQ-12: the exchange price is too old to work out an amount. The
/// counter's words for this end "tell the owner"; these are the owner's.
pub fn stale_rate() -> Words {
    refused(
        "Exchange price too old",
        "The exchange price is too old to work out this amount in bitcoin.",
        "Wait for a new price, or check the internet connection.",
        "RQ-12",
    )
}

/// The same fault from whichever call met it.
fn no_such_request() -> Words {
    refused(
        "Request not found",
        "There is no request with that number, so nothing changed.",
        "Open the list of requests again.",
        "RQ-21",
    )
}

fn address_used() -> Words {
    refused(
        "Address already given",
        "The next address had already been given to another request, so this request was not made.",
        "Try again. Each request gets an address of its own.",
        "RQ-03",
    )
}

fn book_full() -> Words {
    refused(
        "Request book full",
        "The request book holds at most 100,000 requests and 100,000 returns, and it is full.",
        "Keep using the requests already made. This version cannot add more.",
        "RQ-30",
    )
}

/// Words for a request that was not made.
pub fn make_refused(why: &MakeRefused) -> Words {
    match why {
        MakeRefused::NoteNeeded => refused(
            "Note needed",
            "Give the request a name of 1 to 64 characters so you can find it later.",
            "Type a short note.",
            "RQ-06",
        ),
        MakeRefused::BitcoinOnly => refused(
            "Bitcoin only",
            "Your shop runs on the Bitcoin Standard, so its requests are in bitcoin.",
            "Make the request in bitcoin.",
            "RQ-07",
        ),
        MakeRefused::DueOutOfRange => refused(
            "Due date out of range",
            "A request can be due today or on any day up to 366 days from today.",
            "Pick another date.",
            "RQ-09",
        ),
        MakeRefused::TooManyOpen => too_many_open(),
        MakeRefused::TooSmall { min_sats } => too_small(*min_sats),
        MakeRefused::TooLarge { max } => too_large(max),
        MakeRefused::StaleRate => stale_rate(),
        MakeRefused::IndexUsed => address_used(),
        MakeRefused::BookFull => book_full(),
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
pub fn share_refused(why: &ShareRefused) -> Words {
    match why {
        ShareRefused::NoSuchRequest => no_such_request(),
        ShareRefused::NotOpen => refused(
            "Request has ended",
            "This request was cancelled, closed or counted as paid, so it is not sent again.",
            "Make a new request to ask for more.",
            "RQ-12",
        ),
        ShareRefused::StaleRate => stale_rate(),
    }
}

/// Words for payments that were not counted. The bitcoin is in the wallet
/// whatever these say: only the request's count waits.
pub fn credit_refused(why: &CreditRefused) -> Words {
    match why {
        CreditRefused::NoSuchRequest => no_such_request(),
        CreditRefused::NotInBlockOrder => refused(
            "Payments not counted",
            "The payments were not in the order of their blocks, so none was counted. The bitcoin is in your wallet.",
            "Open the request again after the next block.",
            "RQ-15",
        ),
        CreditRefused::NoRate => refused(
            "Payment not counted yet",
            "A payment arrived with no price to count it at, and the exchange price is too old. The bitcoin is in your wallet.",
            "Wait for a new price, or check the internet connection.",
            "RQ-15",
        ),
        CreditRefused::TooManyPayments => refused(
            "Too many payments",
            "This request holds 1,000 payments, the most one request can, so new ones are not counted. The bitcoin is in your wallet.",
            "Check the newest payments in your wallet.",
            "RQ-15",
        ),
    }
}

/// Words for a request that was not cancelled, closed or counted as paid.
pub fn end_refused(why: &EndRefused) -> Words {
    match why {
        EndRefused::NoSuchRequest => no_such_request(),
        EndRefused::AlreadyEnded => refused(
            "Already ended",
            "This request was already cancelled, closed or counted as paid.",
            "Nothing to do.",
            "RQ-19",
        ),
        EndRefused::HasPayments => refused(
            "Something was paid",
            "This request has received a payment, so it cannot be cancelled.",
            "Close it as it is instead.",
            "RQ-19",
        ),
        EndRefused::NotPartPaid => refused(
            "Not part paid",
            "Only a request that is part paid can be closed as it is.",
            "Cancel it if nothing has arrived.",
            "RQ-20",
        ),
        EndRefused::NotAlmost => refused(
            "Not almost paid",
            "Only a request a little short of its amount can be counted as paid.",
            "Close it as it is, or wait for the rest.",
            "RQ-16",
        ),
        EndRefused::NoRepeat => refused(
            "Not repeating",
            "This request does not repeat, so there is nothing to stop.",
            "Nothing to do.",
            "RQ-25",
        ),
    }
}

/// Words for a repeat that was not made.
pub fn repeat_refused(why: &RepeatRefused) -> Words {
    match why {
        RepeatRefused::NotDue => refused(
            "Repeat not due",
            "The next request of this series is not due yet, so it was not made.",
            "Nothing to do. It is made when it is due.",
            "RQ-25",
        ),
        RepeatRefused::TooManyOpen => too_many_open(),
        RepeatRefused::IndexUsed => address_used(),
        RepeatRefused::BookFull => book_full(),
    }
}

/// Words for a return that was not queued. The amounts are written in
/// satoshis.
pub fn return_refused(why: &ReturnRefused) -> Words {
    match why {
        ReturnRefused::NoSuchRequest => no_such_request(),
        ReturnRefused::TooMuch { most_sats } => refused(
            "More than was paid",
            format!(
                "You can return at most {} from this request.",
                sats(*most_sats)
            ),
            "Lower the amount.",
            "RQ-26",
        ),
        ReturnRefused::TooSmall { min_sats } => refused(
            "Too small to send",
            format!(
                "Bitcoin cannot send less than {} to this kind of address.",
                sats(*min_sats)
            ),
            "Return a larger amount.",
            "RQ-26",
        ),
        ReturnRefused::StaleRate => stale_rate(),
        ReturnRefused::BadScript => address_refused(&AddressRefused::NotAnAddress),
        ReturnRefused::QueueFull => refused(
            "Too many returns waiting",
            "99 returns are waiting or in a payment not yet confirmed, the most the queue holds.",
            "Pay the returns, or wait for the last payment to confirm, then add this one.",
            "RQ-29",
        ),
        ReturnRefused::BookFull => book_full(),
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

/// RQ-20: "Close No. {n} as it is?". `arrived` is what the request
/// received, as text.
pub fn close_confirm(number: u64, arrived: &str) -> Confirm {
    Confirm {
        title: format!("Close No. {number} as it is?"),
        text: format!("{arrived} arrived and stays in your books. Nothing more is asked."),
        yes: "Close it",
        no: "Keep it open",
    }
}

/// RQ-27: the step that asks the customer where to return money.
pub fn ask_step() -> Words {
    Words::new(
        Tone::Info,
        "The customer's address",
        "Money is never sent back to where a payment came from, which can be an exchange or someone else.",
        Who::Owner,
        "Ask the customer for an address, then paste or scan their reply.",
    )
    .code("RQ-27")
}

/// RQ-28: the accepted address, shown to compare with the customer.
pub fn check_step() -> Words {
    Words::new(
        Tone::Info,
        "Check the address",
        "Someone who can change your messages can change an address.",
        Who::Owner,
        "Compare it with the customer by phone or in person if you can.",
    )
    .code("RQ-28")
}

/// RQ-29: the returns that wait to be paid.
pub fn returns_waiting(count: usize, amount: &str) -> Words {
    Words::new(
        Tone::Info,
        "Returns waiting",
        format!("{count} returns, {amount} in all, go in one payment from your wallet."),
        Who::Owner,
        "Pay them together when you are ready.",
    )
    .code("RQ-29")
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

/// RQ-37: the line under a request's state when more was returned than it
/// received, after a block left the chain (see `RequestBook::over_returned`);
/// `amount` is the excess as shown.
pub fn over_returned_line(amount: &str) -> String {
    format!("Returned {amount} more than it received")
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
