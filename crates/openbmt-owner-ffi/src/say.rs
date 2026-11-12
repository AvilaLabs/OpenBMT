//! What the owner is told when a call of this layer fails
//! (`docs/SCREEN-RULES.md`, SC-01 to SC-12), the words of the link's states
//! that the link crate leaves without any, and the numbers of the words in
//! the response frames.
//!
//! The words of the owner node's own states and refusals come from the
//! crates that make them (`openbmt_owner_node::say`, `openbmt_owner::say`,
//! `openbmt_watch::say`) and travel with the result or the error. This file
//! holds the words of the errors that this layer makes: a request that is not
//! valid, a handle that does not exist, a fault. [`words_for`] gives the
//! words of a rule string; a string that has none here gets the words of its
//! kind.

use openbmt_core::say::{Tone, Who, Words};
use openbmt_owner_node::Alert;

use crate::error::Kind;

/// The number of a tone in the response frame.
pub(crate) fn tone_byte(tone: Tone) -> u8 {
    match tone {
        Tone::Info => 0,
        Tone::Good => 1,
        Tone::Stop => 2,
        Tone::Ended => 3,
    }
}

/// The number of a person in the response frame.
pub(crate) fn who_byte(who: Who) -> u8 {
    match who {
        Who::Nobody => 0,
        Who::Customer => 1,
        Who::Cashier => 2,
        Who::Manager => 3,
        Who::Owner => 4,
        Who::KeyHolder => 5,
        Who::BoxKeeper => 6,
    }
}

/// Every tone with its name and number, for the protocol description and
/// the Kotlin constants.
pub(crate) const TONES: [(Tone, &str, u8); 4] = [
    (Tone::Info, "Info", 0),
    (Tone::Good, "Good", 1),
    (Tone::Stop, "Stop", 2),
    (Tone::Ended, "Ended", 3),
];

/// Every person with its name and number.
pub(crate) const WHOS: [(Who, &str, u8); 7] = [
    (Who::Nobody, "Nobody", 0),
    (Who::Customer, "Customer", 1),
    (Who::Cashier, "Cashier", 2),
    (Who::Manager, "Manager", 3),
    (Who::Owner, "Owner", 4),
    (Who::KeyHolder, "KeyHolder", 5),
    (Who::BoxKeeper, "BoxKeeper", 6),
];

/// What the app is told to do when its own call was wrong.
const BUG: &str =
    "Try again, and if it happens again, stop and report it to the OpenBMT project; see Details.";

/// One rule string of this layer: the kind, the heading, what happened, who
/// does the next step and what they do.
type Entry = (
    &'static str,
    Kind,
    &'static str,
    &'static str,
    Who,
    &'static str,
);

/// The rule strings this layer makes, each with its own words.
// Words not written yet: a shop with its own wallet has no Key app.
const WORDS_NEEDED_OA_NO_KEY_APP_HEADING: &str = "WORDS NEEDED OA-NO-KEY";
const WORDS_NEEDED_OA_NO_KEY_APP_WHAT: &str = "WORDS NEEDED OA-NO-KEY-APP what.";
const WORDS_NEEDED_OA_NO_KEY_APP_NEXT: &str = "WORDS NEEDED OA-NO-KEY-APP next.";

const WORDS_NEEDED_OA_NO_OWN_WALLET_HEADING: &str = "WORDS NEEDED OA-NO-OWN";
const WORDS_NEEDED_OA_NO_OWN_WALLET_WHAT: &str = "WORDS NEEDED OA-NO-OWN-WALLET what.";
const WORDS_NEEDED_OA_NO_OWN_WALLET_NEXT: &str = "WORDS NEEDED OA-NO-OWN-WALLET next.";

const RULES: &[Entry] = &[
    (
        "watch-file",
        Kind::Refused,
        "Watch file not read",
        "The file the background watcher starts from is not in the form this app reads, so it did not start.",
        Who::Owner,
        "Turn Watch in the background off, then on again.",
    ),
    (
        "watch-key",
        Kind::Refused,
        "Watch key not accepted",
        "The key the background watcher signs with is not a key that can be used, so nothing was done.",
        Who::Owner,
        "Turn Watch in the background off, then on again.",
    ),
    (
        "watch-start",
        Kind::Refused,
        "Watcher did not start",
        "The background watcher did not start, for the reason under Details.",
        Who::Owner,
        "Open the owner app and try again.",
    ),
    (
        "watch-stopped",
        Kind::State,
        "Watcher not running",
        "The background watcher is stopped, so nothing was done.",
        Who::Owner,
        "Open the owner app and turn Watch in the background on again.",
    ),
    (
        "watch-running",
        Kind::State,
        "Watcher already running",
        "The background watcher is running already, so a second one was not started.",
        Who::Owner,
        "Open the owner app and stop the first one.",
    ),
    (
        "watch-no-link",
        Kind::State,
        "No link to watch for",
        "This phone has no link to the counters, so a background watcher would have nobody to tell.",
        Who::Owner,
        "Add a counter, then turn Watch in the background on.",
    ),
    (
        "watch-only",
        Kind::State,
        "Open the owner app",
        "The phone is watching in the background, and the background watcher signs nothing but heartbeats.",
        Who::Owner,
        "Open the owner app to do this.",
    ),
    (
        "request-too-long",
        Kind::Refused,
        "Request too long",
        "The owner app sent the library a request that is longer than it takes, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "request-malformed",
        Kind::Refused,
        "Request not readable",
        "The owner app sent the library a request that is cut short or not in the shape it takes, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "request-magic",
        Kind::Refused,
        "Request not recognised",
        "The owner app sent the library something that is not a request, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "request-version",
        Kind::Refused,
        "Request from another version",
        "The owner app and the library are not of the same version, so nothing was done.",
        Who::Owner,
        "Install the owner app again so that its parts match.",
    ),
    (
        "request-null",
        Kind::Refused,
        "No request",
        "The owner app called the library with no request, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "request-length",
        Kind::Refused,
        "Request has no length",
        "The library could not tell how long the request from the owner app is, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "request-read",
        Kind::Refused,
        "Request not read",
        "The library could not read the request from the owner app, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "op-unknown",
        Kind::Refused,
        "Request not known",
        "The owner app asked the library for something it does not do, so nothing was done.",
        Who::Owner,
        "Install the owner app again so that its parts match.",
    ),
    (
        "handle-unknown",
        Kind::Refused,
        "Owner node not running",
        "The owner app asked about an owner node that is not running, so nothing was done.",
        Who::Owner,
        "Start the owner node again, then repeat the step.",
    ),
    (
        "handle-unexpected",
        Kind::Refused,
        "Request names a node",
        "The owner app named an owner node in a request that works on none, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "start-dir",
        Kind::Refused,
        "No data folder",
        "The owner node was not given a data folder to keep its files in, so it did not start.",
        Who::Owner,
        "Check the owner app's settings for its folder, then start it again.",
    ),
    (
        "start-secret",
        Kind::Refused,
        "Owner key not usable",
        "The key the owner app gave the library to sign with cannot be used, so it did not start.",
        Who::Owner,
        "Restore the owner key from its backup, then start the owner app again.",
    ),
    (
        "start-roots",
        Kind::Refused,
        "Key app identity not usable",
        "The identity of the Key app that this owner app was paired with cannot be used, so it did not start.",
        Who::Owner,
        "Pair the owner app with the Key app again.",
    ),
    (
        "start-wallet",
        Kind::Refused,
        "Wallet not usable",
        "The wallet description that the owner app was given cannot be read for this network, so it did not start.",
        Who::Owner,
        "Set the wallet up again from its backup.",
    ),
    (
        "no-key-app",
        Kind::State,
        WORDS_NEEDED_OA_NO_KEY_APP_HEADING,
        WORDS_NEEDED_OA_NO_KEY_APP_WHAT,
        Who::Owner,
        WORDS_NEEDED_OA_NO_KEY_APP_NEXT,
    ),
    (
        "no-own-wallet",
        Kind::State,
        WORDS_NEEDED_OA_NO_OWN_WALLET_HEADING,
        WORDS_NEEDED_OA_NO_OWN_WALLET_WHAT,
        Who::Owner,
        WORDS_NEEDED_OA_NO_OWN_WALLET_NEXT,
    ),
    (
        "start-key1",
        Kind::Refused,
        "Key 1 not usable",
        "The first of the wallet's keys, kept on this phone, cannot be used, so it did not start.",
        Who::Owner,
        "Restore Key 1 from its backup. See Details.",
    ),
    (
        "start-proxy",
        Kind::Refused,
        "Privacy proxy not usable",
        "The address of the privacy proxy that the owner app was given is not an address, so it did not start.",
        Who::Owner,
        "Check the proxy address in the owner app's settings, then start it again.",
    ),
    (
        "start-peer",
        Kind::Refused,
        "Network peer not usable",
        "A network address that the owner app was given to connect to is not an address, so it did not start.",
        Who::Owner,
        "Check the addresses in the owner app's settings, then start it again.",
    ),
    (
        "start-link",
        Kind::Refused,
        "Counter link not usable",
        "The settings of the link to the counters cannot be used, so the owner node did not start.",
        Who::Owner,
        "Check the relay and the counters in the owner app's settings, then start it again. See Details.",
    ),
    (
        "start-link-files",
        Kind::Store,
        "Link files unavailable",
        "The owner node could not open the files of its link to the counters, so it did not start.",
        Who::Owner,
        "Try again. If this returns, report it to the OpenBMT project and include the Details text.",
    ),
    (
        "owner-files",
        Kind::Store,
        "Owner files unavailable",
        "The owner node could not read a file of its data folder, so it did not start.",
        Who::Owner,
        "Try again. If this returns, report it to the OpenBMT project and include the Details text.",
    ),
    (
        "start-intake",
        Kind::Store,
        "Refund list unavailable",
        "The owner node could not read the list of refund requests from the counters, so it did not start.",
        Who::Owner,
        "Try again. If this returns, report it to the OpenBMT project and include the Details text.",
    ),
    (
        "address-invalid",
        Kind::Refused,
        "Address not usable",
        "The address is not a bitcoin address of this network, so nothing was queued.",
        Who::Owner,
        "Check the address, then enter it again.",
    ),
    (
        "id-length",
        Kind::Refused,
        "Identity not usable",
        "A device identity or a session number is not the 32 bytes it must be, so nothing was done.",
        Who::Owner,
        BUG,
    ),
    (
        "share-range",
        Kind::Refused,
        "Share not usable",
        "A counter's share is more than a counter can be given, so nothing was previewed.",
        Who::Owner,
        BUG,
    ),
    (
        "settings-bytes",
        Kind::Refused,
        "Settings not readable",
        "What was given as the shop's settings does not read as settings, so nothing was changed.",
        Who::Owner,
        BUG,
    ),
    (
        "batch-not-derived",
        Kind::Refused,
        "Batch not made",
        "A batch of addresses for this counter could not be made from the wallet, so nothing was queued.",
        Who::Owner,
        "Check the counter and the range, then try again. See Details.",
    ),
    (
        "link-none",
        Kind::State,
        "No counter link",
        "This owner node was started without a link to the counters, so there is nothing to do here.",
        Who::Owner,
        "Set up the link to the counters in the owner app's settings, then start it again.",
    ),
    (
        "alert-unknown",
        Kind::Refused,
        "Alert not found",
        "The alert is not raised now, so nothing was cleared.",
        Who::Owner,
        "Nothing to do. The alert has gone by itself.",
    ),
    (
        "note-unknown",
        Kind::Refused,
        "Line not found",
        "The line is not in the list of refund requests any more, so nothing was dismissed.",
        Who::Owner,
        "Nothing to do. Read the list again.",
    ),
    (
        "session-unknown",
        Kind::Refused,
        "Approval not found",
        "No open approval has this number, so nothing was done.",
        Who::Owner,
        "Build a new approval if payments still wait in the queue.",
    ),
    (
        "relay-none",
        Kind::State,
        "No Box code to give",
        "The link to the counters has no relay that a counter can reach, so there is no code to give a counter.",
        Who::Owner,
        "Set the Box's address in the owner app's settings, then start it again.",
    ),
    (
        "node-limit",
        Kind::State,
        "Owner node already open",
        "The most owner nodes this app holds are already running, so another was not started.",
        Who::Owner,
        "Stop one of them, then start the owner node again.",
    ),
    (
        "handles-used",
        Kind::State,
        "Out of numbers",
        "The owner app has used every number it can give to a running owner node, so nothing was started.",
        Who::Owner,
        "Close the owner app and open it again.",
    ),
    (
        "start-thread",
        Kind::Internal,
        "Owner node could not start",
        "The library could not make the thread the owner node runs on, so it did not start.",
        Who::Owner,
        BUG,
    ),
    (
        "internal",
        Kind::Internal,
        "Owner fault",
        "The owner library failed in a way it did not expect, so the call was not finished.",
        Who::Owner,
        BUG,
    ),
    (
        "poisoned",
        Kind::Internal,
        "Owner node needs a restart",
        "An earlier call failed in a way the owner library did not expect, so it takes no more calls until it is restarted.",
        Who::Owner,
        "Stop the owner node and start it again. If it happens again, report it to the OpenBMT project.",
    ),
    (
        "pin-wrong",
        Kind::Refused,
        "Wrong PIN",
        "That is not the PIN of this phone's key 1, and nothing was changed.",
        Who::Owner,
        "Check it and try again. Too many wrong tries wipe key 1 from this phone.",
    ),
    (
        "pin-weak",
        Kind::Refused,
        "PIN too short",
        "A PIN is at least 6 digits, or a passphrase of at least 8 characters.",
        Who::Owner,
        "Choose a longer one.",
    ),
    (
        "pin-long",
        Kind::Refused,
        "PIN too long",
        "A PIN or a passphrase is at most 128 characters.",
        Who::Owner,
        "Choose a shorter one.",
    ),
    (
        "OBMx",
        Kind::Refused,
        "Locked setup not readable",
        "Nothing was changed, but the locked setup this phone stored is cut or damaged, so no PIN opens it.",
        Who::Owner,
        "Close the app and open it again. If it stays, follow the recovery kit, or ask whoever holds key 2.",
    ),
    (
        "pin-random",
        Kind::Refused,
        "No random bytes for the lock",
        "The phone's random source did not answer, so nothing was locked and nothing was changed.",
        Who::Owner,
        "Try again. If it happens again, report it to the OpenBMT project; see Details.",
    ),
    (
        "box-address",
        Kind::Refused,
        "Box address not accepted",
        "The address is not an address and a port like 192.168.1.20:8080, or a .onion name of 56 characters and a port, so nothing was sent.",
        Who::Owner,
        "Type the address the Box's own setup page shows, then try again.",
    ),
    (
        "box-unreachable",
        Kind::Refused,
        "Box not reached",
        "This phone could not connect to the Box at that address, or the Box did not answer in time, so nothing was done.",
        Who::Owner,
        "Check the address and that the Box is on. For a .onion address, check that Orbot is running. Then try again.",
    ),
    (
        "box-not-a-box",
        Kind::Refused,
        "Not a Box",
        "What answered at that address is not a Box of this kind, or what it sent is not a claim, so nothing was done.",
        Who::Owner,
        "Check the address on the Box's own setup page, then try again.",
    ),
    (
        "box-no-invitation",
        Kind::Refused,
        "No invitation open",
        "The Box has no open invitation, so it offered no claim. Its invitation runs out after 30 minutes.",
        Who::Owner,
        "Make an invitation on the Key app, type it into the Box's own setup within 30 minutes, then try again.",
    ),
    (
        "box-paired",
        Kind::Refused,
        "Box already paired",
        "This Box is already known to this shop, so it takes no invitation and nothing was queued.",
        Who::Nobody,
        "Nothing to do. To pair it again, erase the Box's data folder first.",
    ),
    (
        "box-wrong-key",
        Kind::Refused,
        "Not the Box that asked",
        "The device at this address is not the Box whose claim the Key app approved, so the welcome was not sent.",
        Who::Owner,
        "Check the address of the Box and try again. The approved settings stay queued.",
    ),
    (
        "box-welcome-refused",
        Kind::Refused,
        "Box did not take the welcome",
        "The Box refused the welcome. Its invitation may have run out or been replaced. The Key app's approval stays queued and nothing is lost.",
        Who::Owner,
        "Make a new invitation on the Key app, type it into the Box, and connect the Box again.",
    ),
    (
        "box-own-invitation",
        Kind::Refused,
        "Invitation not accepted",
        "The invitation text is not one this phone made, so the claim was not checked.",
        Who::Owner,
        "Make a new invitation on this phone and type it into the Box.",
    ),
    (
        "box-claim-mismatch",
        Kind::Refused,
        "Claim does not match",
        "The Box's claim was not made with this phone's invitation, so this phone did not accept the Box.",
        Who::Owner,
        "Make a new invitation, type it into the Box, and try again.",
    ),
    (
        "box-random",
        Kind::Store,
        "No random numbers",
        "This phone's source of random numbers did not answer, so no invitation was made.",
        Who::Owner,
        "Try again. If it fails again, restart the phone.",
    ),
    (
        "deep-none",
        Kind::State,
        "No old addresses to check",
        "No address is set aside, so there is nothing for a check of old addresses to look at.",
        Who::Nobody,
        "Nothing to do. The check is offered when addresses are set aside.",
    ),
];

/// Every rule string of this layer that has words of its own, for the
/// checks of the words and of the sources.
#[cfg(test)]
pub(crate) fn rule_strings() -> impl Iterator<Item = &'static str> {
    RULES.iter().map(|e| e.0)
}

/// What a person is told for an error of this layer: the words of the rule
/// string, or, for a string that has none, the words of its kind.
pub fn words_for(kind: Kind, rule: &str) -> Words {
    if let Some((rule, _, heading, what, who, next)) =
        RULES.iter().find(|e| e.0 == rule && e.1 == kind).copied()
    {
        let tone = match kind {
            Kind::State => Tone::Info,
            Kind::Refused | Kind::Store | Kind::Internal => Tone::Stop,
        };
        return Words::new(tone, heading, what, who, next).detail(rule);
    }
    let (heading, what, next) = match kind {
        Kind::Refused => (
            "Request not accepted",
            "The owner node refused what the app asked, and the reason is not known to this screen.",
            "Try again. If it is refused again, report it to the OpenBMT project; see Details.",
        ),
        Kind::Store => (
            "Owner node cannot save",
            "The owner node could not write to its data folder, so what was asked was not done.",
            "Try once more. If it fails again, report it to the OpenBMT project. See Details.",
        ),
        Kind::State => (
            "Not possible now",
            "What the app asked cannot be done in the state the owner node is in.",
            "Try again in a moment. If it is the same again, report it to the OpenBMT project; see Details.",
        ),
        Kind::Internal => (
            "Owner fault",
            "The owner library failed in a way it did not expect, so the call was not finished.",
            BUG,
        ),
    };
    Words::new(Tone::Stop, heading, what, Who::Owner, next).detail(rule)
}

/// The words of an alert of the link (LK-62). The link crate raises these
/// alerts and leaves their words to the layer that shows them.
pub(crate) fn link_alert_words(alert: &openbmt_owner_node::Alert) -> Words {
    match alert {
        Alert::MissedCheckIn { last_at, .. } => Words::new(
            Tone::Stop,
            "A counter has not checked in",
            "A counter has not reported to this phone for longer than the grace period, so its sales cannot be seen here.",
            Who::Owner,
            "Check that the counter is on, and that it and this phone can reach the relay.",
        )
        .code("LK-62")
        .at(*last_at),
        Alert::Diverged { seq, .. } => Words::new(
            Tone::Stop,
            "A counter's records disagree",
            "The records this phone holds for a counter differ from the ones the counter now reports.",
            Who::Owner,
            "Stop trusting this counter's records and check the counter in person. Clear the alert only when you have.",
        )
        .code("LK-59")
        .detail(format!("record {seq}")),
        Alert::BatchUndelivered { since, .. } => openbmt_owner_node::say::batch_undelivered_words(*since),
    }
}

/// The words of an alert of the link for a shop with its own wallet. A range
/// that a counter has not taken for a day has its own words (OA-112); the
/// others are those of every shop.
pub(crate) fn own_alert_words(alert: &openbmt_owner_node::Alert) -> Words {
    match alert {
        Alert::BatchUndelivered { counter, .. } => {
            openbmt_owner::own_say::range_undelivered(&crate::host::counter_label(counter))
        }
        other => link_alert_words(other),
    }
}

/// The words of what the link holds about one counter.
pub(crate) fn counter_words(
    check_in_seq: Option<u64>,
    held: Option<bool>,
    last_check_in_at: Option<u64>,
) -> Words {
    match (check_in_seq, held) {
        (None, _) => Words::new(
            Tone::Info,
            "No check-in yet",
            "This phone has not taken a check-in from this counter since it started.",
            Who::Nobody,
            "Nothing to do. The counter reports by itself when it is on and the relay is reached.",
        )
        .code("LK-60"),
        (Some(_), Some(false)) => Words::new(
            Tone::Info,
            "Check-in not matched yet",
            "The latest check-in of this counter is not yet matched by the records this phone holds.",
            Who::Nobody,
            "Nothing to do. The records follow with the next messages.",
        )
        .code("LK-60"),
        (Some(_), _) => {
            let words = Words::new(
                Tone::Good,
                "Counter is checking in",
                "The counter's latest check-in is matched by the records this phone holds.",
                Who::Nobody,
                "Nothing to do.",
            )
            .code("LK-60");
            match last_check_in_at {
                Some(at) => words.at(at),
                None => words,
            }
        }
    }
}

/// Every words value this layer can make, for the checks of SC-12.
pub fn all_words() -> Vec<(String, Words)> {
    let mut out: Vec<(String, Words)> = RULES
        .iter()
        .map(|e| (e.0.to_string(), words_for(e.1, e.0)))
        .collect();
    // The words of a kind are those of `internal` for an internal fault.
    for (name, kind) in [
        ("kind-refused", Kind::Refused),
        ("kind-store", Kind::Store),
        ("kind-state", Kind::State),
    ] {
        out.push((name.to_string(), words_for(kind, "a-rule-without-words")));
    }
    let counter = openbmt_core::identity::Identity::from_secret_bytes(&[1; 32])
        .map(|i| i.device_id())
        .ok();
    if let Some(counter) = counter {
        out.push((
            "alert-missed".to_string(),
            link_alert_words(&Alert::MissedCheckIn {
                counter,
                last_at: 1_700_000_000,
            }),
        ));
        out.push((
            "alert-diverged".to_string(),
            link_alert_words(&Alert::Diverged { counter, seq: 3 }),
        ));
        out.push((
            "alert-range-undelivered".to_string(),
            own_alert_words(&Alert::BatchUndelivered {
                counter,
                since: 1_700_000_000,
            }),
        ));
        out.push((
            "alert-batch-undelivered".to_string(),
            link_alert_words(&Alert::BatchUndelivered {
                counter,
                since: 1_700_000_000,
            }),
        ));
    }
    out.push(("counter-none".to_string(), counter_words(None, None, None)));
    out.push((
        "counter-unmatched".to_string(),
        counter_words(Some(4), Some(false), Some(1_700_000_000)),
    ));
    out.push((
        "counter-ok".to_string(),
        counter_words(Some(4), Some(true), Some(1_700_000_000)),
    ));
    out
}
