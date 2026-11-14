//! What the owner is told about a web order (`docs/WEB-ORDER-RULES.md`,
//! `docs/SCREEN-RULES.md`): every refusal, every visit that gave nothing, and
//! what the store said of a list of addresses, each with words of its own.

use openbmt_core::say::{Tone, Who, Words};

use super::check::OrderRefusal;
use super::reply::RangeResult;
use super::WebOrderRefusal;

fn stop(heading: &str, what: &str, next: &str, code: &'static str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::Owner, next).code(code)
}

fn ended(heading: &str, what: &str, next: &str, code: &'static str) -> Words {
    Words::new(Tone::Ended, heading, what, Who::Owner, next).code(code)
}

/// The words for a refusal of a frame, a key or a request.
pub fn refusal_words(refusal: &WebOrderRefusal) -> Words {
    match refusal {
        WebOrderRefusal::Random => stop(
            "Phone could not make a key",
            "This phone's system gave no good random numbers, so no key for the web store was made and nothing was paired.",
            "Restart the phone, then pair the store again.",
            "WO-43",
        ),
        WebOrderRefusal::PairingCode => stop(
            "Pairing code not readable",
            "The pairing code is not complete or was changed, so nothing was paired.",
            "Make a new pairing code on this phone and use that one.",
            "WO-05",
        ),
        WebOrderRefusal::ShopName => stop(
            "Shop name not usable",
            "The shop's name for the store is empty, too long, or has a character that cannot be shown.",
            "Type the shop's name again with letters, numbers and spaces.",
            "WO-05",
        ),
        WebOrderRefusal::PairingId => stop(
            "Addresses for another store",
            "A list of addresses was made for a store paired before this one, so it was not sent.",
            "Unpair the store on this phone and in the store's settings, then pair it again.",
            "WO-10",
        ),
        WebOrderRefusal::Range => stop(
            "Address list not usable",
            "A list of the shop's addresses for the store is not complete or holds something that is not a bitcoin address, so it was not sent.",
            "Visit the store again. If this stays, unpair the store and pair it again.",
            "WO-10",
        ),
        WebOrderRefusal::Notice => stop(
            "Paid notice not usable",
            "A notice that an order is paid was not complete, so it was not sent to the store.",
            "Visit the store again. This phone makes the notice again from its own records.",
            "WO-30",
        ),
        WebOrderRefusal::Signature => stop(
            "Signature does not match",
            "Something signed for the store does not match this phone's key for it, so the store would not take it.",
            "Unpair the store on this phone and in the store's settings, then pair it again.",
            "WO-30",
        ),
        WebOrderRefusal::Request => stop(
            "Request to the store refused",
            "This phone made a request that the store's plugin does not take, so it was not sent.",
            "Update the app and the store's OpenBMT plugin, then visit the store again.",
            "WO-20",
        ),
        WebOrderRefusal::Orders => stop(
            "Store's orders not readable",
            "The store answered with something that is not a list of orders, so no order was taken from it.",
            "Check that the OpenBMT plugin is turned on in the store, then visit the store again.",
            "WO-22",
        ),
    }
}

/// WO-22: the words for an order the owner phone refused and shows.
pub fn order_refusal_words(refusal: &OrderRefusal) -> Words {
    match refusal {
        OrderRefusal::AddressNotSent => stop(
            "Order to an unknown address",
            "The store reported an order with an address this phone never gave it, so the order is not watched.",
            "Do not ship this order. Check who can change the store's site, since only they could do this.",
            "WO-22",
        ),
        OrderRefusal::AddressUsedBefore => stop(
            "Address given twice",
            "The store gave this order an address that another order has already, so this order is not watched.",
            "Do not ship this order yet. Ask the customer to order again, and check who can change the store's site.",
            "WO-22",
        ),
        OrderRefusal::OrderChanged => stop(
            "Order's address changed",
            "The store reported this order before with another address, so the new address is not watched.",
            "Check the order in the store. This phone keeps watching the first address the order was given.",
            "WO-22",
        ),
        OrderRefusal::NoAmount => stop(
            "Order with no amount",
            "The store reported an order of nothing in bitcoin, so this phone does not watch it.",
            "Check the order's total in the store and the store's currency in its settings.",
            "WO-22",
        ),
    }
}

/// WO-22: the words for an order the request book would not take.
pub fn no_request_words() -> Words {
    stop(
        "Order not taken",
        "This phone could not take the order: it is too small or too large, too many requests are open, or the shop's wallet changed.",
        "Check the order's total in the store, and close the requests that are done.",
        "WO-22",
    )
}

/// The words for a visit to the store that gave nothing this phone could use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisitTrouble {
    /// The store could not be reached.
    NotReached,
    /// The shop has Tor on and Tor is not there.
    TorMissing,
    /// The answer was longer than the app reads.
    TooLong,
    /// The store refused the visit.
    Denied,
    /// The store answered with an error.
    Status,
    /// The answer is not the plugin's.
    BadAnswer,
    /// This phone could not take the answer yet.
    NotReady,
    /// An answer came after the phone had moved on.
    Sequence,
}

/// The words for a visit that failed.
pub fn visit_words(trouble: VisitTrouble) -> Words {
    match trouble {
        VisitTrouble::NotReached => ended(
            "Store not reached",
            "This phone could not reach the store at its address, so no new orders were taken.",
            "Check that the store's site opens in a browser on this phone. This phone tries again by itself.",
            "WO-06",
        ),
        VisitTrouble::TorMissing => ended(
            "Tor is not running",
            "The shop has Tor on and Orbot is not answering, so this phone did not visit the store.",
            "Open Orbot and start it. This phone visits the store once Tor answers.",
            "WO-02",
        ),
        VisitTrouble::TooLong => ended(
            "Store's answer too long",
            "The store answered with more than this phone reads at once, so nothing of it was taken.",
            "Visit the store again. If this stays, update the store's OpenBMT plugin.",
            "WO-20",
        ),
        VisitTrouble::Denied => stop(
            "Store refused this phone",
            "The store did not take this phone's visit: it is no longer paired, or its clock and this phone's are more than five minutes apart.",
            "Check the time on this phone and on the store's server. If both are right, pair the store again.",
            "WO-20",
        ),
        VisitTrouble::Status => ended(
            "Store answered with an error",
            "The store's site answered with an error, so no new orders were taken.",
            "Check that the store's site works and the OpenBMT plugin is on. This phone visits again by itself.",
            "WO-20",
        ),
        VisitTrouble::BadAnswer => ended(
            "Answer not from the plugin",
            "The store's site answered, but not as the OpenBMT plugin answers, so nothing of it was taken.",
            "Check that the OpenBMT plugin is installed and turned on in the store, then visit again.",
            "WO-20",
        ),
        VisitTrouble::NotReady => ended(
            "Phone not ready for orders",
            "This phone could not take the store's orders yet: the shop's settings are still loading, or a file could not be stored.",
            "Check that the phone has free storage, then open the app again.",
            "WO-20",
        ),
        VisitTrouble::Sequence => ended(
            "Answer came too late",
            "An answer from the store came after this phone had moved on, so it was not taken.",
            "Nothing to do. This phone visits the store again by itself.",
            "WO-20",
        ),
    }
}

/// What went wrong with the store as this phone keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreTrouble {
    /// The file of web orders could not be read or stored.
    File,
    /// The file holds as many addresses as it may.
    TooMany,
    /// The store's address is not an https address.
    Url,
    /// A store is paired already.
    AlreadyPaired,
    /// No store is paired.
    NotPaired,
    /// The next list of addresses could not be made.
    Indexes,
}

/// The words for the store as this phone keeps it.
pub fn store_words(trouble: StoreTrouble) -> Words {
    match trouble {
        StoreTrouble::File => stop(
            "Web store record not usable",
            "This phone's record of the web store could not be read or stored, so web orders are off.",
            "Check that the phone has free storage and open the app again. If this stays, pair the store again.",
            "WO-12",
        ),
        StoreTrouble::TooMany => stop(
            "Too many store addresses",
            "This phone keeps as many addresses for the store as it can, so it sent the store no more.",
            "Unpair the store and pair it again. The addresses already given stay watched.",
            "WO-12",
        ),
        StoreTrouble::Url => stop(
            "Store address not usable",
            "The store's address must start with https:// and name the store's site, so this one was not saved.",
            "Type the address as it shows in a browser, starting with https://.",
            "WO-06",
        ),
        StoreTrouble::AlreadyPaired => stop(
            "A store is paired already",
            "This phone is paired with one web store, and it takes only one.",
            "Unpair that store first, then pair the new one.",
            "WO-05",
        ),
        StoreTrouble::NotPaired => stop(
            "No store is paired",
            "This phone has no web store paired, so there is nothing to visit or unpair.",
            "Open Web store under Settings and pair the store there.",
            "WO-05",
        ),
        StoreTrouble::Indexes => ended(
            "Addresses not ready",
            "This phone could not make the next list of addresses for the store, so none were sent.",
            "Open the app again. This phone makes the list at its next visit.",
            "WO-10",
        ),
    }
}

/// The words for what the store said of a list of addresses this phone sent.
pub fn range_words(result: RangeResult) -> Words {
    match result {
        RangeResult::Accepted => Words::new(
            Tone::Good,
            "Addresses taken",
            "The store took the addresses this phone sent it.",
            Who::Owner,
            "Nothing to do. Customers can pay in bitcoin at checkout.",
        )
        .code("WO-10"),
        RangeResult::Stale => Words::new(
            Tone::Info,
            "Addresses held already",
            "The store already holds the addresses this phone sent it.",
            Who::Owner,
            "Nothing to do. This phone sends new ones when the store runs low.",
        )
        .code("WO-10"),
        RangeResult::Gap => stop(
            "Addresses skipped some",
            "The store refused the addresses this phone sent because they do not follow its last ones.",
            "Visit the store again. If this stays, unpair the store and pair it again.",
            "WO-10",
        ),
        RangeResult::Overlap => stop(
            "Addresses sent before",
            "The store refused the addresses this phone sent because some of them it was given before.",
            "Visit the store again. This phone sends the list that follows the store's last address.",
            "WO-10",
        ),
        RangeResult::Duplicate => stop(
            "Address the store has",
            "The store refused the addresses this phone sent because one of them is an address it holds already.",
            "Unpair the store on this phone and in the store's settings, then pair it again.",
            "WO-10",
        ),
        RangeResult::WrongPairing => stop(
            "Signed for another pairing",
            "The store refused the addresses this phone sent because they were signed for another pairing.",
            "Unpair the store in its settings and on this phone, then pair it again.",
            "WO-10",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<(String, Words)> {
        let refusals = [
            WebOrderRefusal::Random,
            WebOrderRefusal::PairingCode,
            WebOrderRefusal::ShopName,
            WebOrderRefusal::PairingId,
            WebOrderRefusal::Range,
            WebOrderRefusal::Notice,
            WebOrderRefusal::Signature,
            WebOrderRefusal::Request,
            WebOrderRefusal::Orders,
        ];
        let orders = [
            OrderRefusal::AddressNotSent,
            OrderRefusal::AddressUsedBefore,
            OrderRefusal::OrderChanged,
            OrderRefusal::NoAmount,
        ];
        let visits = [
            VisitTrouble::NotReached,
            VisitTrouble::TorMissing,
            VisitTrouble::TooLong,
            VisitTrouble::Denied,
            VisitTrouble::Status,
            VisitTrouble::BadAnswer,
            VisitTrouble::NotReady,
            VisitTrouble::Sequence,
        ];
        let stores = [
            StoreTrouble::File,
            StoreTrouble::TooMany,
            StoreTrouble::Url,
            StoreTrouble::AlreadyPaired,
            StoreTrouble::NotPaired,
            StoreTrouble::Indexes,
        ];
        let ranges = [
            RangeResult::Accepted,
            RangeResult::Stale,
            RangeResult::Gap,
            RangeResult::Overlap,
            RangeResult::Duplicate,
            RangeResult::WrongPairing,
        ];
        let mut out: Vec<(String, Words)> = Vec::new();
        out.extend(
            refusals
                .iter()
                .map(|r| (format!("{r:?}"), refusal_words(r))),
        );
        out.extend(
            orders
                .iter()
                .map(|r| (format!("{r:?}"), order_refusal_words(r))),
        );
        out.push(("NoRequest".into(), no_request_words()));
        out.extend(visits.iter().map(|v| (format!("{v:?}"), visit_words(*v))));
        out.extend(stores.iter().map(|v| (format!("{v:?}"), store_words(*v))));
        out.extend(ranges.iter().map(|v| (format!("{v:?}"), range_words(*v))));
        out
    }

    /// SC-12: every web order text passes the checks, and no two say the same.
    #[test]
    fn sc_12_the_web_order_words_pass_the_checks_and_differ() {
        let all = all();
        assert_eq!(all.len(), 34);
        for (name, w) in &all {
            assert_eq!(openbmt_core::say::check(w), Ok(()), "{name}");
            assert!(!w.heading.starts_with("WORDS_NEEDED"), "{name}");
        }
        openbmt_core::say::check_distinct(&all).unwrap();
        let mut headings: Vec<&str> = all.iter().map(|(_, w)| w.heading.as_str()).collect();
        headings.sort_unstable();
        headings.dedup();
        assert_eq!(headings.len(), all.len(), "every heading is its own");
    }

    /// WO-22, WO-43: every refusal names the rule it is from.
    #[test]
    fn wo_22_wo_43_the_words_name_their_rule() {
        assert_eq!(refusal_words(&WebOrderRefusal::Random).code, Some("WO-43"));
        assert_eq!(
            order_refusal_words(&OrderRefusal::AddressNotSent).code,
            Some("WO-22")
        );
        assert_eq!(visit_words(VisitTrouble::NotReached).code, Some("WO-06"));
    }
}
