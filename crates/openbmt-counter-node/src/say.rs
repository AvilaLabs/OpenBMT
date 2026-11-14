//! What the owner is told when a counter will not start or pairing stops,
//! and the list of every word the counter program can show
//! (`docs/SCREEN-RULES.md`, SC-01 to SC-12).
//!
//! The owner reads these while setting up a counter. The cashier's own words
//! are in `openbmt_counter::say`, the instant-acceptance words in
//! `openbmt_core::say_policy` and the watcher's in `openbmt_watch::say`;
//! [`program_words`] joins the four and [`screens_markdown`] writes them as
//! one table (`docs/screens/counter.md`).

use openbmt_core::approval::ChangeClass;
use openbmt_core::pairing::PairError;
use openbmt_core::pairing_own::OwnPairError;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::settings::SettingsError;

use crate::confirm::{ChangeNotice, ConfirmRefused};
use crate::dirstore::DirStoreError;
use crate::node::{HeartbeatNotTaken, StartError};
use crate::paired::{PairRefused, RelayRefused};
use crate::paired_own::OwnPairRefused;
use crate::relaylink::LinkLine;
use crate::settings::{SettingsFault, SettingsLine, SettingsRefused};
use crate::starter::StarterRefused;
use crate::shopln::{SeedNotTaken, Why};
use crate::switch::{SwitchNotice, SwitchRefused, SwitchStepRefused};
use openbmt_lightning_node::room::OnChainReason;

impl DirStoreError {
    /// What the owner is told about a file of the counter's data folder.
    pub fn words(&self) -> Words {
        let (heading, what, next) = match self {
            Self::Io { .. } => (
                "Cannot use data folder",
                "The operating system refused an action on a file in the counter's data folder, so it did not start.",
                "Check that the folder exists, has room and may be written to, then start again. See Details.",
            ),
            Self::Corrupt { .. } => (
                "Data file unreadable",
                "A file in the counter's data folder does not read back as it was written, so it did not start.",
                "Restart the counter. If it stays, the file needs to be put back from a copy. See Details.",
            ),
            Self::RecordTooLarge(_) => (
                "Saved record too large",
                "A saved record is longer than its file takes, so the counter did not start.",
                "Restart the counter. If it stays, the saved records need a look. See Details.",
            ),
        };
        Words::new(Tone::Stop, heading, what, Who::Owner, next).detail(self.to_string())
    }
}

impl StartError {
    /// What the owner is told when the counter will not start. Nothing was
    /// changed in any of these cases that the sentence does not say.
    pub fn words(&self) -> Words {
        match self {
            Self::Locked(e) => Words::new(
                Tone::Stop,
                "Counter already running",
                "Another copy of the counter, or another start, is using this counter's data folder.",
                Who::Owner,
                "Close the other copy and start the counter again.",
            )
            .code("NR-79")
            .detail(e.clone()),
            Self::Dir(e) => e.words(),
            Self::Headers(e) => Words::new(
                Tone::Stop,
                "Saved headers unreadable",
                "The counter's saved block headers could not be read at all, so it did not start.",
                Who::Owner,
                "Restart the counter. If it stays, the saved headers need to be replaced. See Details.",
            )
            .detail(e.clone()),
            Self::Counter(e) => e.words(),
            Self::FirstBatch(e) => Words::new(
                Tone::Stop,
                "First addresses not taken",
                "The first batch of payment codes the owner signed was not taken, so the counter did not start.",
                Who::Owner,
                "Check that the counter was given the owner's batch, then start it again. See Details.",
            )
            .detail(e.clone()),
            Self::Checked(e) => Words::new(
                Tone::Stop,
                "Checked height not saved",
                "The counter could not write how far it has checked the chain, so it did not start.",
                Who::Owner,
                "Check that the data folder has room and may be written to, then start again. See Details.",
            )
            .detail(e.to_string()),
            Self::Settings(e) => e.words(),
            Self::Driver(e) => Words::new(
                Tone::Stop,
                "Network part did not start",
                "The part of the counter that connects to the network did not start, so the counter did not start.",
                Who::Owner,
                "Check the counter's internet connection and start it again. See Details.",
            )
            .detail(e.clone()),
            Self::Switch(e) => said(Tone::Stop, "CR_261_START", Who::Owner)
                .code("CR-261")
                .detail(e.clone()),
        }
    }
}

/// The words of section 26 (CR-264), by the key of their rule.
fn said(tone: Tone, key: &'static str, who: Who) -> Words {
    let (heading, what, next) = match key {
        "CR_261_START" => ("Switch not finished at start", "The counter stopped in the middle of switching to the new wallet and could not finish it, so it did not start.", "Start the counter again. If it stops again, see Details."),
        "CR_257_NOT_A_SWITCH" => ("Not a wallet switch", "The message from the owner phone is not a switch this counter can read, so nothing changed.", "Send the switch again from the owner phone."),
        "CR_263_NOT_STARTER" => ("Not a starter counter", "This counter was not paired with a starter wallet, so it takes no switch. Nothing changed.", "To pay into another wallet, pair this counter again."),
        "CR_258_NOT_FROM_OWNER" => ("Switch not from the owner", "The switch is not signed by the owner phone this counter was paired with, so nothing changed.", "Send the switch from the shop's own owner phone."),
        "CR_258_WRONG_COUNTER" => ("Switch for another counter", "The switch names another counter, so nothing changed here.", "Send each counter its own switch from the owner phone."),
        "CR_258_WRONG_WALLET" => ("Switch from another wallet", "The switch starts from a wallet this counter does not pay into, so nothing changed.", "Send the switch again from the owner phone."),
        "CR_258_NEW_WALLET" => ("New wallet does not read", "The new wallet in the switch is not one this counter can use, so nothing changed.", "Connect the new wallet on the owner phone again, then send the switch."),
        "CR_258_SAME_WALLET" => ("Already this wallet", "The switch names the wallet this counter already pays into, so nothing changed.", "No switch is needed."),
        "CR_258_RANGE" => ("Switch addresses refused", "The addresses that come with the switch are not ones this counter may use, so nothing changed.", "Send the switch again from the owner phone. See Details."),
        "CR_259_NOT_SAVED" => ("Switch not saved", "The counter could not save the switch, so nothing changed.", "Check that the counter has room, then send the switch again."),
        "CR_261_NOTHING_WAITS" => ("No switch waits", "There is no new wallet to check on this counter.", "Nothing to do."),
        "CR_261_NO_RECORD" => ("Pairing not readable", "The counter has no pairing record to update, so nothing changed.", "Pair this counter again with the shop's owner phone."),
        "CR_261_NOT_WRITTEN" => ("Switch not finished yet", "The counter could not write every part of the switch. It finishes the switch at its next start.", "Restart the counter."),
        "CR_259_DROPPED" => ("Wallet check expired", "The new wallet waited seven days without a check, so it was dropped. Sales still go to the starter wallet.", "Send the switch again from the owner phone when a manager can check it."),
        "CR_261_SWITCHED" => ("Switched to the new wallet", "From now on this counter's sales go to the new wallet.", "Nothing more to do on this counter."),
        "CR_260_DIFFERENT" => ("New wallet not taken", "The manager said the address on the counter differs from the new wallet's own screen, so this counter keeps paying into the starter wallet.", "Check that the right wallet was connected on the owner phone. If it was, the owner phone may not be safe to use."),
        "CR_260_HOME_LINE" => ("A new wallet waits", "The owner phone sent a new wallet for this counter's sales.", "Open New wallet to check in the manager's menu."),
        _ => (key, key, key),
    };
    Words::new(tone, heading, what, who, next)
}

/// Words of section 26 that are still to be written: the placeholder stands
/// in the heading and both sentences, so that it shows wherever the words
/// would (CR-276). Its value is its own name, which names the rule.
fn needed(tone: Tone, placeholder: &'static str, who: Who) -> Words {
    Words::new(tone, placeholder, placeholder, who, placeholder)
}

/// Shop Lightning's words (SL-20 to SL-77), by the name of their state.
fn shopln_said(tone: Tone, key: &'static str, who: Who) -> Words {
    let (heading, what, next) = match key {
        "WORDS_NEEDED_SL76_NO_SEED" => ("Lightning key not scanned", "Shop Lightning runs on this counter, but its Lightning key has not been scanned here yet. Sales take on-chain payments only.", "Scan the stick's Lightning key in the manager's menu, under Lightning key."),
        "WORDS_NEEDED_SL41_STARTING" => ("Lightning is starting", "The counter is starting Shop Lightning. Until it is ready, sales take on-chain payments only.", "Wait a moment. Lightning is offered once it has started."),
        "WORDS_NEEDED_SL60_MAINNET" => ("No Lightning on real bitcoin", "Shop Lightning is not ready for real bitcoin in this release, so this counter takes on-chain payments only.", "Nothing needs doing. Sales go on with on-chain payments."),
        "WORDS_NEEDED_SL33_NO_WALLET" => ("No wallet to move money to", "Shop Lightning moves its money to the shop's own wallet, and this counter was paired without one.", "Turn Shop Lightning off on the owner phone, or pair this counter again with the shop's own wallet."),
        "WORDS_NEEDED_SL73_NOT_THE_BOX" => ("Not connected to the Box", "Shop Lightning keeps a second copy on the shop's Box, and this counter was paired to reach the shop another way.", "Pair this counter again through the shop's Box. Until then, sales take on-chain payments only."),
        "WORDS_NEEDED_SL32_NO_ROUTE" => ("Tor not reached", "The shop's settings send Lightning through Tor, and this counter cannot reach Tor, so Lightning has not started.", "Check the counter's network, then restart the counter."),
        "WORDS_NEEDED_SL41_NOT_STARTED" => ("Lightning did not start", "Shop Lightning could not start on this counter, so sales take on-chain payments only.", "Restart the counter. If Lightning still does not start, see Details."),
        "WORDS_NEEDED_SL77_CLOSING" => ("Lightning is closing", "Shop Lightning was turned off or moved, so this counter is closing its channels and offers no Lightning.", "Keep the counter on and online until the money is back in the shop's wallet."),
        "WORDS_NEEDED_SL41_SECOND_DEVICE_AWAY" => ("Box not answering", "The Box keeps Shop Lightning's second copy and is not answering, so this counter offers no Lightning for now.", "Check that the Box is on and connected. Lightning comes back by itself when it answers."),
        "WORDS_NEEDED_SL41_CHAIN_BEHIND" => ("Lightning catching up", "The counter has not caught up with the newest blocks yet, so it offers no Lightning for now.", "Keep the counter online for a few minutes. Lightning comes back by itself."),
        "WORDS_NEEDED_SL40_NO_ROOM" => ("Too much for Lightning now", "This amount is more than Lightning can take in right now, so the sale shows the on-chain code only.", "Take this payment on-chain. Smaller sales can still be paid by Lightning."),
        "WORDS_NEEDED_SL41_NO_AMOUNT" => ("No amount for Lightning", "A Lightning payment needs an amount above zero, and this sale has none yet.", "Enter the amount of the sale."),
        "WORDS_NEEDED_SL41_PROVIDER_AWAY" => ("Lightning provider away", "No channel can take this payment and the shop's Lightning provider is not answering, so the sale shows the on-chain code only.", "Take this payment on-chain. Lightning comes back by itself when the provider answers."),
        "WORDS_NEEDED_SL29_FEE_ABOVE_MAX" => ("Opening fee above the limit", "Paying by Lightning would open a new channel, and the provider asks more for that than the shop's most for one opening.", "Take this payment on-chain, or raise the most for one opening on the owner phone."),
        "WORDS_NEEDED_SL30_WINDOW_TOO_SHORT" => ("Provider not used", "The provider gives the shop less than a week to come back online after a problem with a channel, so Lightning is not used.", "Pick another provider on the owner phone. Until then, sales take on-chain payments only."),
        "WORDS_NEEDED_SL20_NOT_STORED" => ("Invoice not kept twice", "The Lightning invoice could not be kept on both this counter and the Box, so it is not shown.", "Take this payment on-chain."),
        "WORDS_NEEDED_SL41_LIBRARY" => ("Invoice not made", "Shop Lightning could not make an invoice for this sale, so it shows the on-chain code only.", "Take this payment on-chain. If it happens again, see Details."),
        "WORDS_NEEDED_SL76_NOT_HERE" => ("Not the Lightning counter", "The shop's settings do not run Shop Lightning on this counter, so it takes no Lightning key. Nothing was kept.", "Scan the key on the counter the owner phone picked for Lightning."),
        "WORDS_NEEDED_SL76_NOT_A_KEY" => ("Not a Lightning key", "The code scanned is not the stick's Lightning key, so nothing was kept.", "Scan the code the stick shows for the Lightning key."),
        "WORDS_NEEDED_SL72_WRONG_NODE" => ("Wrong Lightning key", "This key does not match the one the owner phone set when it turned Shop Lightning on, so nothing was kept.", "Scan the Lightning key from the stick the owner phone read when Lightning was turned on."),
        "WORDS_NEEDED_SL76_ALREADY_STORED" => ("Lightning key already kept", "This counter already keeps a Lightning key, so nothing changed.", "Nothing more to do."),
        "WORDS_NEEDED_SL76_SCAN_THE_KEY" => ("Scan the Lightning key", "The stick shows the Lightning key as a code. This counter checks it against the shop's settings before keeping it.", "Hold the stick's code in front of the camera."),
        "WORDS_NEEDED_SL76_KEY_KEPT" => ("Lightning key kept", "The key matches the shop's settings and is kept on this counter only. Lightning starts now.", "Put the stick back in its safe place."),
        _ => (key, key, key),
    };
    Words::new(tone, heading, what, who, next)
}

/// SL-41, SL-60, SL-73, SL-76, SL-77: what a manager is told while Lightning
/// is not offered on this counter, or not offered for a sale. The node's own
/// text is in the detail.
pub fn shopln_words(why: &Why) -> Words {
    use openbmt_lightning_node::words as node_words;
    let (key, rule, detail) = match why {
        Why::NoSeed => ("WORDS_NEEDED_SL76_NO_SEED", "SL-76", None),
        Why::Starting => ("WORDS_NEEDED_SL41_STARTING", "SL-41", None),
        Why::Mainnet => (
            "WORDS_NEEDED_SL60_MAINNET",
            "SL-60",
            Some(node_words::SL_60.to_string()),
        ),
        Why::NoWallet => ("WORDS_NEEDED_SL33_NO_WALLET", "SL-33", None),
        Why::NotTheBox => ("WORDS_NEEDED_SL73_NOT_THE_BOX", "SL-73", None),
        Why::NoRoute => ("WORDS_NEEDED_SL32_NO_ROUTE", "SL-32", None),
        Why::NotStarted(e) => ("WORDS_NEEDED_SL41_NOT_STARTED", "SL-41", Some(e.clone())),
        Why::Closing => ("WORDS_NEEDED_SL77_CLOSING", "SL-77", None),
        Why::Node(OnChainReason::SecondDeviceAway) => (
            "WORDS_NEEDED_SL41_SECOND_DEVICE_AWAY",
            "SL-41",
            Some(node_words::SL_23.to_string()),
        ),
        Why::Node(OnChainReason::ChainBehind) => ("WORDS_NEEDED_SL41_CHAIN_BEHIND", "SL-41", None),
        Why::Node(OnChainReason::NoRoom) => (
            "WORDS_NEEDED_SL40_NO_ROOM",
            "SL-40",
            Some(node_words::SL_40.to_string()),
        ),
        Why::Node(OnChainReason::NoAmount) => ("WORDS_NEEDED_SL41_NO_AMOUNT", "SL-41", None),
        Why::Node(OnChainReason::ProviderAway) => (
            "WORDS_NEEDED_SL41_PROVIDER_AWAY",
            "SL-41",
            Some(node_words::SL_41.to_string()),
        ),
        Why::Node(OnChainReason::FeeAboveMax) => (
            "WORDS_NEEDED_SL29_FEE_ABOVE_MAX",
            "SL-29",
            Some(node_words::SL_29.to_string()),
        ),
        Why::Node(OnChainReason::WindowTooShort) => (
            "WORDS_NEEDED_SL30_WINDOW_TOO_SHORT",
            "SL-30",
            Some(node_words::SL_30.to_string()),
        ),
        Why::NotStored => (
            "WORDS_NEEDED_SL20_NOT_STORED",
            "SL-20",
            Some(node_words::SL_20_INVOICE_NOT_STORED.to_string()),
        ),
        Why::Library(e) => ("WORDS_NEEDED_SL41_LIBRARY", "SL-41", Some(e.clone())),
    };
    let words = shopln_said(Tone::Info, key, Who::Manager).code(rule);
    match detail {
        Some(d) => words.detail(d),
        None => words,
    }
}

impl SeedNotTaken {
    /// SL-72, SL-76: what a manager is told when a scanned Lightning key was
    /// not taken. Nothing was stored.
    pub fn words(&self) -> Words {
        let key = match self {
            Self::NotHere => "WORDS_NEEDED_SL76_NOT_HERE",
            Self::NotASeed => "WORDS_NEEDED_SL76_NOT_A_KEY",
            Self::WrongNode(_) => "WORDS_NEEDED_SL72_WRONG_NODE",
            Self::AlreadyStored => "WORDS_NEEDED_SL76_ALREADY_STORED",
        };
        shopln_said(Tone::Stop, key, Who::Manager)
            .code(self.rule())
            .detail(self.to_string())
    }
}

/// SL-76: the words of the screen that scans the Lightning key and of the
/// line that says it was kept.
pub fn shopln_scan_words() -> Words {
    shopln_said(Tone::Info, "WORDS_NEEDED_SL76_SCAN_THE_KEY", Who::Manager).code("SL-76")
}

/// SL-76: the key was taken.
pub fn shopln_seed_kept_words() -> Words {
    shopln_said(Tone::Good, "WORDS_NEEDED_SL76_KEY_KEPT", Who::Manager).code("SL-76")
}

/// Every word of Shop Lightning with its name, for the list of every word.
pub fn all_shopln_words() -> Vec<(String, Words)> {
    let mut all = Vec::new();
    for (name, why) in [
        ("NoSeed", Why::NoSeed),
        ("Starting", Why::Starting),
        ("Mainnet", Why::Mainnet),
        ("NoWallet", Why::NoWallet),
        ("NotTheBox", Why::NotTheBox),
        ("NoRoute", Why::NoRoute),
        ("NotStarted", Why::NotStarted(String::new())),
        ("Closing", Why::Closing),
        (
            "SecondDeviceAway",
            Why::Node(OnChainReason::SecondDeviceAway),
        ),
        ("ChainBehind", Why::Node(OnChainReason::ChainBehind)),
        ("NoRoom", Why::Node(OnChainReason::NoRoom)),
        ("NoAmount", Why::Node(OnChainReason::NoAmount)),
        ("ProviderAway", Why::Node(OnChainReason::ProviderAway)),
        ("FeeAboveMax", Why::Node(OnChainReason::FeeAboveMax)),
        ("WindowTooShort", Why::Node(OnChainReason::WindowTooShort)),
        ("NotStored", Why::NotStored),
        ("Library", Why::Library(String::new())),
    ] {
        all.push((format!("ShopLightning::{name}"), shopln_words(&why)));
    }
    let wrong = openbmt_core::shop_lightning::SeedRefused::WrongNode {
        pinned: [0; 33],
        scanned: [1; 33],
    };
    for (name, e) in [
        ("NotHere", SeedNotTaken::NotHere),
        ("NotASeed", SeedNotTaken::NotASeed),
        ("WrongNode", SeedNotTaken::WrongNode(wrong)),
        ("AlreadyStored", SeedNotTaken::AlreadyStored),
    ] {
        all.push((format!("SeedNotTaken::{name}"), e.words()));
    }
    all.push(("ShopLightning::scan".to_string(), shopln_scan_words()));
    all.push(("ShopLightning::kept".to_string(), shopln_seed_kept_words()));
    all
}

/// Whether these words are a placeholder (CR-264).
pub fn is_placeholder(words: &Words) -> bool {
    words.heading.starts_with("WORDS_NEEDED_")
}

impl SwitchRefused {
    /// What is told about a switch that was not taken to be checked (CR-257,
    /// CR-258, CR-259, CR-263). Nothing changed.
    pub fn words(&self) -> Words {
        let (placeholder, who) = match self {
            Self::Frame(_) => ("CR_257_NOT_A_SWITCH", Who::Owner),
            Self::NotStarter => ("CR_263_NOT_STARTER", Who::Owner),
            Self::NotFromOwner => ("CR_258_NOT_FROM_OWNER", Who::Owner),
            Self::WrongCounter => ("CR_258_WRONG_COUNTER", Who::Owner),
            Self::WrongWallet => ("CR_258_WRONG_WALLET", Who::Owner),
            Self::NewWalletRefused => ("CR_258_NEW_WALLET", Who::Owner),
            Self::SameWallet => ("CR_258_SAME_WALLET", Who::Owner),
            Self::Range(_) => ("CR_258_RANGE", Who::Owner),
            Self::NotWritten(_) => ("CR_259_NOT_SAVED", Who::Owner),
        };
        said(Tone::Ended, placeholder, who)
            .code(self.rule())
            .detail(self.to_string())
    }
}

impl SwitchStepRefused {
    /// What a manager is told when the answer to a waiting switch did not go
    /// through (CR-260, CR-261).
    pub fn words(&self) -> Words {
        let (tone, placeholder) = match self {
            Self::NothingWaits => (Tone::Ended, "CR_261_NOTHING_WAITS"),
            Self::NoRecord => (Tone::Stop, "CR_261_NO_RECORD"),
            Self::Refused(e) => return e.words(),
            Self::NotWritten(_) => (Tone::Stop, "CR_261_NOT_WRITTEN"),
        };
        said(tone, placeholder, Who::Manager)
            .code(self.rule())
            .detail(self.to_string())
    }
}

impl SwitchNotice {
    /// What a manager is told about a switch that came to nothing (CR-259,
    /// CR-264).
    pub fn words(&self) -> Words {
        match self {
            Self::Dropped => said(Tone::Ended, "CR_259_DROPPED", Who::Manager).code("CR-259"),
        }
    }
}

/// CR-261: what a manager is told when the counter switched.
pub fn switched_words() -> Words {
    said(Tone::Good, "CR_261_SWITCHED", Who::Nobody).code("CR-261")
}

/// CR-260: what a manager is told after "Different".
pub fn switch_dropped_words() -> Words {
    said(Tone::Ended, "CR_260_DIFFERENT", Who::Owner).code("CR-260")
}

/// CR-260: the line Home shows the manager while a switch waits.
pub fn switch_home_line_words() -> Words {
    said(Tone::Info, "CR_260_HOME_LINE", Who::Manager).code("CR-260")
}

impl StarterRefused {
    /// What is told about a starter line that was not taken, or a payment
    /// that was not marked seen (CR-272, CR-274, CR-276). Nothing changed.
    pub fn words(&self) -> Words {
        let (placeholder, who) = match self {
            Self::Frame(_) => ("WORDS_NEEDED_CR272_NOT_A_LINE", Who::Owner),
            Self::NotStarter => ("WORDS_NEEDED_CR272_NOT_STARTER", Who::Owner),
            Self::NotFromOwner => ("WORDS_NEEDED_CR272_NOT_FROM_OWNER", Who::Owner),
            Self::Old { .. } => ("WORDS_NEEDED_CR272_OLD", Who::Owner),
            Self::NotWritten(_) => ("WORDS_NEEDED_CR272_NOT_SAVED", Who::Owner),
            Self::NoSuchPayment => ("WORDS_NEEDED_CR274_NO_SUCH_PAYMENT", Who::Cashier),
        };
        needed(Tone::Ended, placeholder, who)
            .code(self.rule())
            .detail(self.to_string())
    }
}

/// CR-273, SS-38: what "New sale" says when it offers no bitcoin because the
/// starter wallet holds the limit and the owner asked to stop above it. (The
/// owner phone's `no_bitcoin_words` in `openbmt-owner` are words for this
/// place.)
pub fn starter_stopped_words() -> Words {
    needed(Tone::Info, "WORDS_NEEDED_CR273_STOPPED", Who::Cashier).code("CR-273")
}

/// CR-273: the line Home shows when the last starter line said stop and is
/// too old to say whether the wallet is over the limit.
pub fn starter_unchecked_words() -> Words {
    needed(Tone::Info, "WORDS_NEEDED_CR273_UNCHECKED", Who::Manager).code("CR-273")
}

/// CR-274: the heading of the cards of Home that show the starter wallet's
/// payments.
pub fn starter_cards_words() -> Words {
    needed(Tone::Info, "WORDS_NEEDED_CR274_CARDS", Who::Manager).code("CR-274")
}

impl SettingsRefused {
    /// What the owner is told when settings the owner sent were not taken
    /// (CR-160, CR-161). Nothing was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Bytes(e) => Words::new(
                Tone::Ended,
                "Settings not readable",
                "What was sent as the shop's settings is not settings, so nothing was changed.",
                Who::Owner,
                "Nothing to do. The owner's phone sends them again by itself.",
            )
            .detail(e.to_string()),
            Self::Verify(SettingsError::ConfirmationRequired(ChangeClass::Redirect)) => {
                needs_wallet()
            }
            Self::Confirmation(_) | Self::NoConfirmationHere => bad_confirmation(),
            Self::Verify(e) => Words::new(
                Tone::Ended,
                "Settings refused",
                "The settings the owner sent are not signed as this counter needs, or do not go past the ones it has, so nothing was changed.",
                Who::Owner,
                "Check on the owner's phone that the change was approved on the Key app, then send it again.",
            )
            .detail(e.to_string()),
            Self::Store(e) => Words::new(
                Tone::Ended,
                "Settings not saved",
                "The counter could not write the owner's new settings to its data folder, so it kept the old ones.",
                Who::Owner,
                "Check that the data folder has room and may be written to. The owner's phone sends them again by itself.",
            )
            .detail(e.to_string()),
        }
        .code(self.rule())
    }
}

impl SettingsFault {
    /// What the owner is told when the counter will not start for what it
    /// holds of the settings (CR-163).
    pub fn words(&self) -> Words {
        let restore = "Put the counter's data folder back from a copy made after the last settings change, or pair the counter again from the Key app.";
        match self {
            Self::Read(e) => Words::new(
                Tone::Stop,
                "Saved settings not read",
                "The counter could not read the settings it saved from the owner, so it did not start.",
                Who::Owner,
                "Check that the data folder may be read, then start again. If it stays, restore it from a copy.",
            )
            .detail(e.to_string()),
            Self::Decode(e) => Words::new(
                Tone::Stop,
                "Saved settings unreadable",
                "The settings the counter saved from the owner do not read back, so it did not start.",
                Who::Owner,
                restore,
            )
            .detail(e.to_string()),
            Self::Verify(e) => Words::new(
                Tone::Stop,
                "Saved settings not valid",
                "The settings the counter saved from the owner are not signed as they must be, so it did not start.",
                Who::Owner,
                restore,
            )
            .detail(e.to_string()),
            Self::Rolled { .. } => Words::new(
                Tone::Stop,
                "Saved settings are old",
                "The counter's records show newer settings than the ones it saved, so it did not start rather than use old limits.",
                Who::Owner,
                restore,
            )
            .detail(self.to_string()),
        }
        .code(self.rule())
    }
}

/// CR-239: a change that sends payments somewhere else needs the wallet, not
/// a counter.
fn needs_wallet() -> Words {
    Words::new(
        Tone::Ended,
        "Change needs the wallet",
        "This change sends payments somewhere else, which only the shop's wallet can approve. That is not built yet, so nothing changed.",
        Who::Owner,
        "Keep this setting as it is for now.",
    )
    .code("CR-239")
}

/// CR-242: the confirmation that came with a change is not a counter's of
/// this shop.
fn bad_confirmation() -> Words {
    Words::new(
        Tone::Ended,
        "Confirmation refused",
        "The change came with a confirmation that is not from a counter of this shop, so nothing changed.",
        Who::Manager,
        "Ask the owner to have the change confirmed at a counter again.",
    )
    .code("CR-242")
}

impl ChangeNotice {
    /// What a manager is told about a change that came to nothing (CR-239,
    /// CR-242, CR-244).
    pub fn words(&self) -> Words {
        match self {
            Self::Dropped { .. } => Words::new(
                Tone::Ended,
                "Change not confirmed in time",
                "A change from the owner phone waited seven days and was dropped. Nothing changed.",
                Who::Manager,
                "Ask the owner to make the change again.",
            )
            .code("CR-239"),
            Self::BadConfirmation { .. } => bad_confirmation(),
            Self::NeedsWallet { .. } => needs_wallet(),
        }
    }
}

/// CR-241: what a manager is told when the confirmation was signed.
pub fn change_confirmed_words() -> Words {
    Words::new(
        Tone::Good,
        "Change confirmed",
        "The owner phone sends the change to every counter. It applies once it arrives.",
        Who::Nobody,
        "Nothing to do.",
    )
    .code("CR-241")
}

impl ConfirmRefused {
    /// What a manager is told when the confirmation was not signed.
    pub fn words(&self) -> Words {
        let words = match self {
            Self::NothingWaits => Words::new(
                Tone::Ended,
                "Nothing to confirm",
                "No change waits at this counter, or a newer one has taken its place, so nothing was confirmed.",
                Who::Manager,
                "Open Change to confirm again to see what waits now.",
            ),
            Self::NotWritten(_) => Words::new(
                Tone::Stop,
                "Confirmation not saved",
                "The counter could not save its confirmation, so nothing was confirmed and nothing changed.",
                Who::Manager,
                "Try again. If it fails again, tell the owner.",
            ),
        };
        words.code(self.rule()).detail(self.to_string())
    }
}

impl SettingsLine {
    /// What Home says about the settings (CR-164, CR-165).
    pub fn words(&self) -> Words {
        match self {
            Self::Removed => Words::new(
                Tone::Stop,
                "Counter removed",
                "The owner removed this counter from the shop. It cannot start a sale, but sales already open can finish.",
                Who::Manager,
                "Ask the owner. To use this counter again the owner must add it and pair it again.",
            )
            .code("CR-165"),
            Self::RestartNeeded => Words::new(
                Tone::Info,
                "Restart for settings",
                "The owner changed Tor, a node the shop uses or how payments are checked, which only take effect when the counter starts again.",
                Who::Nobody,
                "Nothing to do. The counter starts again by itself when no sale screen is open.",
            )
            .code("CR-164"),
        }
    }
}

impl HeartbeatNotTaken {
    /// What the owner is told when the counter did not take a check-in.
    pub fn words(&self) -> Words {
        match self {
            Self::Bytes(e) => Words::new(
                Tone::Ended,
                "Check-in not readable",
                "What was sent as a check-in is not a check-in, so nothing was changed.",
                Who::Owner,
                "Nothing to do. The owner's phone sends a new check-in by itself.",
            )
            .code("CR-50")
            .detail(e.to_string()),
            Self::Refused(e) => e.words(),
        }
    }
}

/// The words for a step of pairing that stopped. Nothing was paired.
pub fn pair_error_words(e: &PairError) -> Words {
    let stop = |heading: &str, what: &str, next: &str| {
        Words::new(Tone::Ended, heading, what, Who::Owner, next)
    };
    match e {
        PairError::SameRoot => stop(
            "Owner and Key app are alike",
            "The owner's phone and the Key app are the same key, so pairing stopped.",
            "Set the Key app up on a different device from the owner's phone, then pair again.",
        ),
        PairError::CounterIsRoot => stop(
            "Counter key is not its own",
            "This counter's key is the owner's or the Key app's, so pairing stopped.",
            "Set this counter up again with a key of its own, then pair again.",
        ),
        PairError::InvalidName => stop(
            "Counter name not allowed",
            "The counter's name must be 1 to 32 characters of plain text, so pairing stopped.",
            "Choose another name and pair again.",
        ),
        PairError::UnsupportedNetwork => stop(
            "Network not supported",
            "The Key app's offer names a bitcoin network that a counter cannot be paired on, so pairing stopped.",
            "Use a Key app set up on the main network, a test network or regtest, then pair again.",
        ),
        PairError::Decode(inner) => stop(
            "Pairing code unreadable",
            "The scanned pairing code could not be read, so pairing stopped.",
            "Show the code again and scan it again.",
        )
        .detail(inner.to_string()),
        PairError::BindingMismatch => stop(
            "Request is for other keys",
            "The counter's request was made for other keys or another wallet than the Key app's offer, so pairing stopped.",
            "Start pairing again from the Key app and scan its new offer.",
        ),
        PairError::RequestSignatureInvalid => stop(
            "Request not from counter",
            "The request's signature does not match the counter's key, so pairing stopped.",
            "Start pairing again.",
        ),
        PairError::NotConfirmed => stop(
            "Words not confirmed",
            "The words on the two screens were not confirmed as the same, so pairing stopped.",
            "Start pairing again and check that both screens show the same four words.",
        ),
        PairError::ClockWentBack => stop(
            "Counter's clock went back",
            "The counter's clock is earlier than when pairing started, so pairing stopped.",
            "Check the counter's clock and pair again.",
        ),
        PairError::Expired => stop(
            "Pairing took too long",
            "Pairing took longer than ten minutes, so it stopped.",
            "Start pairing again and finish within ten minutes.",
        ),
        PairError::WrongWallet => stop(
            "Pairing for another wallet",
            "The pairing answer is for a different wallet than the one this counter was offered, so pairing stopped.",
            "Start pairing again with the right Key app.",
        ),
        PairError::WrongCounter => stop(
            "Pairing for another counter",
            "The pairing answer names a different counter, so pairing stopped.",
            "Start pairing again and scan the answer on the counter that showed the request.",
        ),
        PairError::WrongNonce => stop(
            "Pairing answer is old",
            "The pairing answer belongs to an earlier try, so pairing stopped.",
            "Start pairing again.",
        ),
        PairError::WrongName => stop(
            "Pairing name differs",
            "The pairing answer carries a different counter name, so pairing stopped.",
            "Start pairing again.",
        ),
        PairError::ApproverSignatureInvalid => stop(
            "Key app signature rejected",
            "The Key app's signature on the pairing answer does not check out, so pairing stopped.",
            "Start pairing again with the right Key app.",
        ),
        PairError::OwnerSignatureInvalid => stop(
            "Owner signature rejected",
            "The owner's signature on the pairing answer does not check out, so pairing stopped.",
            "Start pairing again with the right owner's phone.",
        ),
        PairError::NotTheApprover => stop(
            "Wrong Key app",
            "This is not the Key app named in the offer, so pairing stopped.",
            "Use the Key app that showed the offer.",
        ),
        PairError::NotTheOwner => stop(
            "Wrong owner phone",
            "This is not the owner's phone named in the offer, so pairing stopped.",
            "Use the owner's phone named in the offer.",
        ),
    }
}

/// The steps of pairing, as the counter's screen tells the owner (the
/// ceremony in `openbmt_core::pairing`, CR-91 to CR-99).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairStep {
    /// The Key app shows an offer and the counter scans it.
    ScanOffer,
    /// The counter shows its code and four words, and waits for the person
    /// to compare them with the Key app's.
    CompareWords,
    /// The person said the words match; the counter scans the owner's phone.
    ScanOwner,
    /// The counter has taken the signed pairing.
    Paired,
    /// The person said the words differ; the counter forgot the pairing.
    Differ,
    /// The person said the address differs from the wallet's first receive
    /// address in a pairing with the shop's own wallet (CR-228); the counter
    /// forgot the pairing before it made a key.
    AddressDiffers,
    /// The person said the four words differ in a pairing with the shop's own
    /// wallet (CR-229); the counter forgot the pairing.
    OwnDiffer,
}

impl PairStep {
    /// Every step.
    pub const ALL: [PairStep; 7] = [
        Self::ScanOffer,
        Self::CompareWords,
        Self::ScanOwner,
        Self::Paired,
        Self::Differ,
        Self::AddressDiffers,
        Self::OwnDiffer,
    ];

    /// The name of the step.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ScanOffer => "ScanOffer",
            Self::CompareWords => "CompareWords",
            Self::ScanOwner => "ScanOwner",
            Self::Paired => "Paired",
            Self::Differ => "Differ",
            Self::AddressDiffers => "AddressDiffers",
            Self::OwnDiffer => "OwnDiffer",
        }
    }

    /// What the owner is told at this step.
    pub fn words(&self) -> Words {
        match self {
            Self::ScanOffer => Words::new(
                Tone::Info,
                "Scan the Key app's code",
                "The counter is waiting for the offer that the Key app shows on its Pair a counter screen.",
                Who::Owner,
                "Hold the counter's camera to the Key app's screen.",
            ),
            Self::CompareWords => Words::new(
                Tone::Info,
                "Compare the four words",
                "The counter shows a code and four words. The owner's phone carries the code to the Key app, which shows four words too.",
                Who::Owner,
                "Scan this code with the owner's phone. If the Key app shows the same four words, tap They match here.",
            ),
            Self::ScanOwner => Words::new(
                Tone::Info,
                "Scan the owner's phone",
                "The words match. The counter waits for the code the owner's phone shows once the Key app has approved.",
                Who::Owner,
                "Hold the counter's camera to the owner's phone and keep it there until the code is read.",
            ),
            Self::Paired => Words::new(
                Tone::Good,
                "Counter is paired",
                "The counter took the pairing signed by the Key app and the owner's phone, and the shop's settings.",
                Who::Nobody,
                "Nothing to do. If no addresses came with it, the owner's phone sends them next.",
            ),
            Self::Differ => Words::new(
                Tone::Stop,
                "Words differ: stopped",
                "The four words did not match, so the counter forgot its key and its number and did not pair. Someone may have changed the code on the way.",
                Who::Owner,
                "Do not approve it on the Key app. Start again from the Key app: open Pair a counter and scan its code.",
            )
            .code("CR-92"),
            Self::AddressDiffers => Words::new(
                Tone::Stop,
                "Different wallet",
                "The address on this counter is not your wallet's first address, so the owner phone gave this counter a different wallet. Nothing was saved.",
                Who::Manager,
                "Check that the owner phone has your wallet, then start the pairing again from the owner phone.",
            )
            .code("CR-228"),
            Self::OwnDiffer => Words::new(
                Tone::Stop,
                "Words differ: stopped",
                "The four words did not match, so the counter forgot its key and did not pair. Someone may have changed a code on the way.",
                Who::Manager,
                "Tap They differ on the owner phone too, then start again from it.",
            )
            .code("CR-229"),
        }
    }
}

impl PairRefused {
    /// What the owner is told when a step of pairing, or a saved pairing, is
    /// not accepted. Nothing was paired.
    pub fn words(&self) -> Words {
        let again = "Start again from the Key app: open Pair a counter and scan its code.";
        let stop = |heading: &str, what: &str, next: &str| {
            Words::new(Tone::Ended, heading, what, Who::Owner, next).code(self.rule())
        };
        match self {
            Self::NoCeremony => stop(
                "No pairing in progress",
                "The counter has no pairing under way. It may have been closed, restarted or used up, which forgets a pairing that was not finished.",
                again,
            ),
            Self::NoRandomness => stop(
                "Counter could not make a key",
                "The phone gave the counter no random numbers it can trust, so it did not make a key and pairing did not start.",
                "Try again. If it fails again, restart the phone and then pair from the Key app.",
            ),
            Self::Pair(e) => pair_error_words(e).code(self.rule()),
            Self::Bundle(e) => stop(
                "Owner code not readable",
                "The code from the owner's phone is not what the counter expects to finish pairing, so pairing is not finished.",
                "Show the code on the owner's phone again and scan it again.",
            )
            .detail(e.to_string()),
            Self::RelayNotSigned => stop(
                "Relay not signed",
                "The relay in the code from the owner's phone is not signed by that owner, so pairing stopped.",
                "Start again from the Key app: open Pair a counter and scan its code.",
            ),
            Self::Settings(e) => stop(
                "Settings not signed",
                "The shop's settings that came with the pairing are not signed by both the owner's phone and the Key app, so pairing stopped.",
                again,
            )
            .detail(e.to_string()),
            Self::SettingsOld { .. } => stop(
                "Settings are too old",
                "The shop's settings that came with the pairing are older than the pairing itself, so pairing stopped.",
                again,
            ),
            Self::NotOnDevices => stop(
                "Counter not in settings",
                "The shop's settings do not list this counter, so pairing stopped.",
                "Have the owner's phone add this counter to the settings, then start again from the Key app.",
            ),
            Self::NotOnCounters => stop(
                "Counter has no share",
                "The shop's settings list this counter as a device but not among the counters that take payments, so pairing stopped.",
                "Have the owner's phone give this counter a share in the settings, then start again from the Key app.",
            ),
            Self::Batch(e) => stop(
                "First addresses refused",
                "The first batch of addresses does not fit this counter or this wallet, or is smaller than a counter takes, so pairing stopped.",
                "Have the owner's phone send a batch made for this counter, then start again from the Key app.",
            )
            .detail(e.to_string()),
            Self::Record(e) => stop(
                "Saved pairing not readable",
                "The pairing this counter saved does not read back, so the counter did not start.",
                "Restore the counter from its backup, or pair it again from the Key app.",
            )
            .detail(e.to_string()),
            Self::RecordKey => stop(
                "Saved pairing has no key",
                "The pairing this counter saved holds a secret that is not a key, so the counter did not start.",
                "Restore the counter from its backup, or pair it again from the Key app.",
            ),
        }
    }
}

impl OwnPairRefused {
    /// What the person at the counter or the owner is told when a step of
    /// pairing with the shop's own wallet, or a saved one, is not accepted
    /// (CR-227 to CR-233). Nothing was paired. A refusal whose words are
    /// those of a refusal of the Key app's ceremony says them.
    pub fn words(&self) -> Words {
        match self {
            Self::NoCeremony => stopped("Pairing not under way", "This counter has no pairing with the shop's wallet at this step. Closing or restarting the app forgets one not yet finished.", Who::Manager, "Start again from the owner phone: open Add a counter and scan its code."),
            Self::NoRandomness => stopped("No key made", "The phone gave no random numbers the counter can trust, so it made no key and the pairing with the shop's wallet did not start.", Who::Manager, "Try again. If it fails again, restart the phone and start again from the owner phone."),
            Self::Pair(e) => own_pair_error_words(e),
            Self::Bundle(e) => PairRefused::Bundle(e.clone()).words(),
            Self::Settings(_) => stopped("Settings not signed", "The first settings in the owner phone's code are not signed by that owner phone, or are not valid. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
            Self::SettingsOld { .. } => stopped("Settings out of date", "The first settings in the owner phone's code are older than the ones it offered. Nothing was saved.", Who::Manager, "Update the owner phone, then start the pairing again."),
            Self::NotOnDevices => stopped("Counter not in the settings", "The first settings from the owner phone do not list this counter, so it could not take them. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
            Self::NotOnCounters => stopped("Counter not set up to sell", "The first settings from the owner phone do not set this counter up to take sales. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
            Self::RelayNotSigned => stopped("Relay not the owner's", "The relay in the owner phone's code is not signed by that owner phone, so it may not be the shop's. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
            Self::Record(_) => stopped("Saved pairing damaged", "The pairing with the shop's wallet that this counter saved does not read back, so the counter did not start.", Who::Owner, "Put the counter's files back from a copy, or pair it again from the owner phone."),
            Self::RecordKey => stopped("Saved key damaged", "The pairing with the shop's wallet that this counter saved holds no usable key, so the counter did not start.", Who::Owner, "Put the counter's files back from a copy, or pair it again from the owner phone."),
        }
        .code(self.rule())
        .detail(self.to_string())
    }
}

/// Words for a step of an own-wallet pairing that stopped.
fn stopped(heading: &str, what: &str, who: Who, next: &str) -> Words {
    Words::new(Tone::Stop, heading, what, who, next)
}

/// The words for a step of an own-wallet pairing that stopped (CR-227 to
/// CR-232).
pub fn own_pair_error_words(e: &OwnPairError) -> Words {
    // The refusals the two ceremonies share say the same words.
    let shared = |e: PairError| pair_error_words(&e);
    match e {
        OwnPairError::NotMainNetwork => stopped("Not for real bitcoin", "The owner phone's code is for a test network, not for real bitcoin, so this counter did not pair.", Who::Manager, "Pair with an owner phone set up for real bitcoin."),
        OwnPairError::InvalidName => shared(PairError::InvalidName),
        OwnPairError::CounterIsOwner => stopped("Counter key refused", "The key this counter made is the owner phone's own key, which a counter may not have. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
        OwnPairError::Decode(inner) => shared(PairError::Decode(inner.clone())),
        OwnPairError::BindingMismatch => stopped("Codes do not belong together", "The counter's code was made for a different code from the owner phone. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone and scan its newest code."),
        OwnPairError::RequestSignatureInvalid => shared(PairError::RequestSignatureInvalid),
        OwnPairError::NotConfirmed => shared(PairError::NotConfirmed),
        OwnPairError::ClockWentBack => shared(PairError::ClockWentBack),
        OwnPairError::Expired => shared(PairError::Expired),
        OwnPairError::WrongWallet => Words::new(
            Tone::Stop,
            "Wrong wallet",
            "The owner phone's code is for a different wallet than the one this counter checked. Nothing was saved.",
            Who::Manager,
            "Start the pairing again from the owner phone.",
        ),
        OwnPairError::WrongCounter => shared(PairError::WrongCounter),
        OwnPairError::WrongNonce => shared(PairError::WrongNonce),
        OwnPairError::WrongName => shared(PairError::WrongName),
        OwnPairError::OwnerSignatureInvalid => shared(PairError::OwnerSignatureInvalid),
        OwnPairError::NotTheOwner => shared(PairError::NotTheOwner),
        OwnPairError::Bundle(_) => shared(PairError::Decode(
            openbmt_core::wire::DecodeError::NonCanonical("the peers"),
        )),
        OwnPairError::Range(_) => stopped("Addresses not signed", "The addresses the owner phone gave this counter are not signed for it, or are not a proper range. Nothing was saved.", Who::Manager, "Start the pairing again from the owner phone."),
        OwnPairError::RangeTooSmall { .. } => Words::new(
            Tone::Stop,
            "Too few addresses",
            "The owner phone gave this counter fewer than 1,000 addresses, the least it takes. Nothing was saved.",
            Who::Manager,
            "Update the owner phone, then start the pairing again.",
        ),
    }
}

impl LinkLine {
    /// What the counter's screen says about the link to the owner (CR-136).
    /// The link never holds up a sale, and the words say so.
    pub fn words(&self) -> Words {
        match *self {
            Self::Connecting => Words::new(
                Tone::Info,
                "Connecting to the Box",
                "The counter is reaching the owner through the shop's Box. Sales do not wait for it.",
                Who::Nobody,
                "Nothing to do. This line changes when the Box answers.",
            ),
            Self::Connected => Words::new(
                Tone::Info,
                "Connected to the Box",
                "The counter can send its records to the owner and take the owner's heartbeat.",
                Who::Nobody,
                "Nothing to do.",
            ),
            Self::NotReachable { since } => {
                let words = Words::new(
                    Tone::Info,
                    "Box not reachable",
                    "The counter cannot reach the Box, so it cannot send its records to the owner or take the owner's heartbeat.",
                    Who::Owner,
                    "Check that the Box is on and on the same network. The counter tries again by itself.",
                );
                match since {
                    Some(at) => words.at(at),
                    None => words,
                }
            }
            Self::Refused => Words::new(
                Tone::Info,
                "Box refused this counter",
                "The Box answered but did not accept this counter, so nothing goes to the owner through it.",
                Who::Owner,
                "Tell the owner. The owner's phone must list this counter in the shop's settings.",
            ),
            Self::NeedsTor => Words::new(
                Tone::Info,
                "Orbot needed for the Box",
                "The Box is reached over Tor and Orbot is not running, so the counter cannot reach it.",
                Who::Manager,
                "Open Orbot and turn it on. The counter tries again by itself.",
            ),
            Self::NotStarted => Words::new(
                Tone::Info,
                "Box link did not start",
                "The counter could not open the files of its link to the Box, so it does not talk to the owner. Sales are not affected.",
                Who::Manager,
                "Close the counter and open it again. If it stays, tell the owner.",
            ),
        }
        .code("CR-136")
    }
}

impl RelayRefused {
    /// What the owner is told when the counter does not take a relay for
    /// counters (CR-132). Nothing was changed.
    pub fn words(&self) -> Words {
        match self {
            Self::Bytes(e) => Words::new(
                Tone::Ended,
                "Box code not readable",
                "The code is not what the counter expects for the owner's Box, so nothing was changed.",
                Who::Owner,
                "Show the Box code on the owner's phone again and scan it again.",
            )
            .detail(e.to_string()),
            Self::NotFromOwner => Words::new(
                Tone::Ended,
                "Box code not from the owner",
                "The code was not signed by the owner's phone this counter was paired with, so nothing was changed.",
                Who::Owner,
                "Show the Box code on the owner's own phone and scan it again.",
            ),
        }
        .code(self.rule())
    }
}

/// One value of every line the link can show.
pub fn every_link_line() -> Vec<(&'static str, LinkLine)> {
    vec![
        ("Connecting", LinkLine::Connecting),
        ("Connected", LinkLine::Connected),
        (
            "NotReachable",
            LinkLine::NotReachable {
                since: Some(1_700_000_000),
            },
        ),
        ("NotReachableNoTime", LinkLine::NotReachable { since: None }),
        ("Refused", LinkLine::Refused),
        ("NeedsTor", LinkLine::NeedsTor),
        ("NotStarted", LinkLine::NotStarted),
    ]
}

/// One value of every refusal of a relay for counters.
pub fn every_relay_refused() -> Vec<(&'static str, RelayRefused)> {
    use openbmt_core::wire::DecodeError;
    vec![
        ("Bytes", RelayRefused::Bytes(DecodeError::Truncated)),
        ("NotFromOwner", RelayRefused::NotFromOwner),
    ]
}

/// Every one of the words that only this crate gives, for SC-12: one value
/// for every variant of the start errors, of the data folder errors, of the
/// pairing errors and of the pairing steps.
pub fn all_words() -> Vec<(String, Words)> {
    use openbmt_core::wire::DecodeError;
    use openbmt_counter::refusal::{HeartbeatRefused, LoadError};
    let mut all: Vec<(String, Words)> = Vec::new();
    let io = || DirStoreError::Io {
        file: "records.bin".to_string(),
        op: "write",
        kind: std::io::ErrorKind::PermissionDenied,
    };
    let corrupt = || DirStoreError::Corrupt {
        file: "desk.bin".to_string(),
        why: "the file is cut short",
    };
    for (name, e) in [
        ("Io", io()),
        ("Corrupt", corrupt()),
        ("RecordTooLarge", DirStoreError::RecordTooLarge(9_999_999)),
    ] {
        all.push((
            format!("StartError::Dir/{name}"),
            StartError::Dir(e).words(),
        ));
    }
    // CR-112 to CR-116: the words of the Lightning driver.
    all.extend(crate::lightning::all_words());
    all.extend(crate::swap::all_words());
    // SL-41, SL-72, SL-76, SL-77: the words of Shop Lightning.
    all.extend(all_shopln_words());
    for (name, e) in [
        ("NothingWaits", crate::confirm::ConfirmRefused::NothingWaits),
        (
            "NotWritten",
            crate::confirm::ConfirmRefused::NotWritten("records.bin: full".to_string()),
        ),
    ] {
        all.push((format!("ConfirmRefused::{name}"), e.words()));
    }
    for (name, e) in crate::ownnode::every_setting_refusal() {
        all.push((format!("OwnNodeSettingRefused::{name}"), e.words()));
    }
    let starts = [
        (
            "Locked",
            StartError::Locked("the folder is locked".to_string()),
        ),
        ("Headers", StartError::Headers("cut short".to_string())),
        (
            "Counter",
            StartError::Counter(LoadError::Desk(DecodeError::Truncated)),
        ),
        (
            "FirstBatch",
            StartError::FirstBatch("not signed".to_string()),
        ),
        ("Checked", StartError::Checked(io())),
        ("Driver", StartError::Driver("no network".to_string())),
    ];
    for (name, e) in starts {
        all.push((format!("StartError::{name}"), e.words()));
    }
    all.push((
        "HeartbeatNotTaken::Bytes".to_string(),
        HeartbeatNotTaken::Bytes(DecodeError::Truncated).words(),
    ));
    all.push((
        "HeartbeatNotTaken::Refused".to_string(),
        HeartbeatNotTaken::Refused(HeartbeatRefused::Store(openbmt_counter::StoreFault::Write(
            corrupt(),
        )))
        .words(),
    ));
    for (name, e) in every_pair_error() {
        all.push((format!("PairError::{name}"), pair_error_words(&e)));
    }
    for step in PairStep::ALL {
        all.push((format!("PairStep::{}", step.name()), step.words()));
    }
    for (name, e) in every_pair_refused() {
        all.push((format!("PairRefused::{name}"), e.words()));
    }
    // CR-227 to CR-233: only the words that the Key app's ceremony does not
    // already say are listed again; the shared ones are listed above.
    for (name, e) in every_own_pair_refused() {
        if own_words_are_new(&e) {
            all.push((format!("OwnPairRefused::{name}"), e.words()));
        }
    }
    for (name, line) in every_link_line() {
        // The one that has no time is the same state as the one that has:
        // listed once.
        if name != "NotReachableNoTime" {
            all.push((format!("LinkLine::{name}"), line.words()));
        }
    }
    for (name, e) in every_relay_refused() {
        all.push((format!("RelayRefused::{name}"), e.words()));
    }
    for (name, e) in every_settings_refused() {
        all.push((format!("SettingsRefused::{name}"), e.words()));
    }
    for (name, e) in every_settings_fault() {
        all.push((format!("SettingsFault::{name}"), e.words()));
    }
    for line in [SettingsLine::Removed, SettingsLine::RestartNeeded] {
        all.push((format!("SettingsLine::{line:?}"), line.words()));
    }
    // CR-239, CR-241: the dropped change and the confirmation signed. The
    // other two notices say what a refusal of settings says and are listed
    // there.
    all.push((
        "ChangeNotice::Dropped".to_string(),
        ChangeNotice::Dropped { version: 2 }.words(),
    ));
    all.push(("ChangeConfirmed".to_string(), change_confirmed_words()));
    // CR-264: the states and refusals of a switch from the starter wallet.
    all.push((
        "StartError::Switch".to_string(),
        StartError::Switch("the ranges are for another wallet".to_string()).words(),
    ));
    for (name, e) in every_switch_refused() {
        all.push((format!("SwitchRefused::{name}"), e.words()));
    }
    for (name, e) in every_switch_step_refused() {
        // The refusal that wraps another has the words of what it wraps.
        if name != "Refused" {
            all.push((format!("SwitchStepRefused::{name}"), e.words()));
        }
    }
    all.push((
        "SwitchNotice::Dropped".to_string(),
        SwitchNotice::Dropped.words(),
    ));
    all.push(("SwitchHomeLine".to_string(), switch_home_line_words()));
    // CR-276: the states and refusals of the starter line.
    for (name, e) in every_starter_refused() {
        all.push((format!("StarterRefused::{name}"), e.words()));
    }
    all.push(("StarterStopped".to_string(), starter_stopped_words()));
    all.push(("StarterUnchecked".to_string(), starter_unchecked_words()));
    all.push(("StarterCards".to_string(), starter_cards_words()));
    all.push(("Switched".to_string(), switched_words()));
    all.push(("SwitchDifferent".to_string(), switch_dropped_words()));
    all
}

/// One value for every variant of [`SwitchRefused`] (SC-12).
pub fn every_switch_refused() -> Vec<(&'static str, SwitchRefused)> {
    use openbmt_core::pinned_wallet::RangeError;
    use openbmt_core::wire::DecodeError;
    vec![
        ("Frame", SwitchRefused::Frame(DecodeError::Truncated)),
        ("NotStarter", SwitchRefused::NotStarter),
        ("NotFromOwner", SwitchRefused::NotFromOwner),
        ("WrongCounter", SwitchRefused::WrongCounter),
        ("WrongWallet", SwitchRefused::WrongWallet),
        ("NewWalletRefused", SwitchRefused::NewWalletRefused),
        ("SameWallet", SwitchRefused::SameWallet),
        (
            "Range",
            SwitchRefused::Range(OwnPairError::Range(RangeError::OverlapsUsed)),
        ),
        (
            "NotWritten",
            SwitchRefused::NotWritten("switch.bin: full".to_string()),
        ),
    ]
}

/// One value for every variant of [`StarterRefused`] (SC-12).
pub fn every_starter_refused() -> Vec<(&'static str, StarterRefused)> {
    use openbmt_core::wire::DecodeError;
    vec![
        ("Frame", StarterRefused::Frame(DecodeError::Truncated)),
        ("NotStarter", StarterRefused::NotStarter),
        ("NotFromOwner", StarterRefused::NotFromOwner),
        ("Old", StarterRefused::Old { last: 5, got: 4 }),
        (
            "NotWritten",
            StarterRefused::NotWritten("starter-line.bin: full".to_string()),
        ),
        ("NoSuchPayment", StarterRefused::NoSuchPayment),
    ]
}

/// One value for every variant of [`SwitchStepRefused`] (SC-12).
pub fn every_switch_step_refused() -> Vec<(&'static str, SwitchStepRefused)> {
    vec![
        ("NothingWaits", SwitchStepRefused::NothingWaits),
        ("NoRecord", SwitchStepRefused::NoRecord),
        (
            "Refused",
            SwitchStepRefused::Refused(SwitchRefused::NotStarter),
        ),
        (
            "NotWritten",
            SwitchStepRefused::NotWritten("ranges.bin: full".to_string()),
        ),
    ]
}

/// Whether the words of an own-wallet refusal are its own and not those of a
/// refusal of the Key app's ceremony.
pub fn own_words_are_new(e: &OwnPairRefused) -> bool {
    match e {
        OwnPairRefused::Pair(inner) => matches!(
            inner,
            OwnPairError::NotMainNetwork
                | OwnPairError::CounterIsOwner
                | OwnPairError::BindingMismatch
                | OwnPairError::WrongWallet
                | OwnPairError::Range(_)
                | OwnPairError::RangeTooSmall { .. }
        ),
        OwnPairRefused::Bundle(_) => false,
        _ => true,
    }
}

/// One value of every refusal of an own-wallet pairing step, with its name.
pub fn every_own_pair_refused() -> Vec<(&'static str, OwnPairRefused)> {
    use openbmt_core::pinned_wallet::RangeError;
    use openbmt_core::wire::DecodeError;
    vec![
        ("NoCeremony", OwnPairRefused::NoCeremony),
        ("NoRandomness", OwnPairRefused::NoRandomness),
        (
            "Pair/NotMainNetwork",
            OwnPairRefused::Pair(OwnPairError::NotMainNetwork),
        ),
        (
            "Pair/CounterIsOwner",
            OwnPairRefused::Pair(OwnPairError::CounterIsOwner),
        ),
        (
            "Pair/BindingMismatch",
            OwnPairRefused::Pair(OwnPairError::BindingMismatch),
        ),
        (
            "Pair/WrongWallet",
            OwnPairRefused::Pair(OwnPairError::WrongWallet),
        ),
        (
            "Pair/Range",
            OwnPairRefused::Pair(OwnPairError::Range(RangeError::BadSignature)),
        ),
        (
            "Pair/RangeTooSmall",
            OwnPairRefused::Pair(OwnPairError::RangeTooSmall {
                got: 999,
                need: 1_000,
            }),
        ),
        ("Bundle", OwnPairRefused::Bundle(DecodeError::Truncated)),
        (
            "Settings",
            OwnPairRefused::Settings(SettingsError::OwnerSignatureInvalid),
        ),
        (
            "SettingsOld",
            OwnPairRefused::SettingsOld { floor: 4, got: 4 },
        ),
        ("NotOnDevices", OwnPairRefused::NotOnDevices),
        ("NotOnCounters", OwnPairRefused::NotOnCounters),
        ("RelayNotSigned", OwnPairRefused::RelayNotSigned),
        ("Record", OwnPairRefused::Record(DecodeError::Truncated)),
        ("RecordKey", OwnPairRefused::RecordKey),
    ]
}

/// One value of every refusal of settings from the owner, with its name.
pub fn every_settings_refused() -> Vec<(&'static str, SettingsRefused)> {
    use openbmt_core::wire::DecodeError;
    vec![
        ("Bytes", SettingsRefused::Bytes(DecodeError::Truncated)),
        (
            "Verify",
            SettingsRefused::Verify(SettingsError::ApproverRequired),
        ),
        (
            "ConfirmationRequired",
            SettingsRefused::Verify(SettingsError::ConfirmationRequired(ChangeClass::Redirect)),
        ),
        (
            "Confirmation",
            SettingsRefused::Confirmation(openbmt_core::approval::ConfirmError::NotACounter),
        ),
        (
            "Store",
            SettingsRefused::Store(DirStoreError::Io {
                file: "settings.bin".to_string(),
                op: "write",
                kind: std::io::ErrorKind::PermissionDenied,
            }),
        ),
    ]
}

/// One value of every fault of the settings a start reads, with its name.
pub fn every_settings_fault() -> Vec<(&'static str, SettingsFault)> {
    use openbmt_core::wire::DecodeError;
    vec![
        (
            "Read",
            SettingsFault::Read(DirStoreError::Io {
                file: "settings.bin".to_string(),
                op: "read",
                kind: std::io::ErrorKind::PermissionDenied,
            }),
        ),
        ("Decode", SettingsFault::Decode(DecodeError::Truncated)),
        (
            "Verify",
            SettingsFault::Verify(SettingsError::OwnerSignatureInvalid),
        ),
        (
            "Rolled",
            SettingsFault::Rolled {
                in_force: 3,
                recorded: 5,
            },
        ),
    ]
}

/// One value of every refusal of the pairing steps, with its name. The
/// `Pair` variant carries a pairing error, whose words are listed with the
/// pairing errors.
pub fn every_pair_refused() -> Vec<(&'static str, PairRefused)> {
    use openbmt_core::batch::BatchError;
    use openbmt_core::wire::DecodeError;
    vec![
        ("NoCeremony", PairRefused::NoCeremony),
        ("NoRandomness", PairRefused::NoRandomness),
        ("Bundle", PairRefused::Bundle(DecodeError::Truncated)),
        (
            "Settings",
            PairRefused::Settings(SettingsError::ApproverRequired),
        ),
        ("SettingsOld", PairRefused::SettingsOld { floor: 4, got: 4 }),
        ("RelayNotSigned", PairRefused::RelayNotSigned),
        ("NotOnDevices", PairRefused::NotOnDevices),
        ("NotOnCounters", PairRefused::NotOnCounters),
        (
            "Batch",
            PairRefused::Batch(BatchError::CountTooSmall { min: 1_000, got: 3 }),
        ),
        ("Record", PairRefused::Record(DecodeError::Truncated)),
        ("RecordKey", PairRefused::RecordKey),
    ]
}

/// One value of every pairing error, with its name.
pub fn every_pair_error() -> Vec<(&'static str, PairError)> {
    use openbmt_core::wire::DecodeError;
    vec![
        ("SameRoot", PairError::SameRoot),
        ("CounterIsRoot", PairError::CounterIsRoot),
        ("InvalidName", PairError::InvalidName),
        ("UnsupportedNetwork", PairError::UnsupportedNetwork),
        ("Decode", PairError::Decode(DecodeError::Truncated)),
        ("BindingMismatch", PairError::BindingMismatch),
        (
            "RequestSignatureInvalid",
            PairError::RequestSignatureInvalid,
        ),
        ("NotConfirmed", PairError::NotConfirmed),
        ("ClockWentBack", PairError::ClockWentBack),
        ("Expired", PairError::Expired),
        ("WrongWallet", PairError::WrongWallet),
        ("WrongCounter", PairError::WrongCounter),
        ("WrongNonce", PairError::WrongNonce),
        ("WrongName", PairError::WrongName),
        (
            "ApproverSignatureInvalid",
            PairError::ApproverSignatureInvalid,
        ),
        ("OwnerSignatureInvalid", PairError::OwnerSignatureInvalid),
        ("NotTheApprover", PairError::NotTheApprover),
        ("NotTheOwner", PairError::NotTheOwner),
    ]
}

/// Every word the counter program can show: the counter's, this crate's, the
/// watcher's and the policy enums'. A value whose heading and first sentence
/// are already in the list under another name (the policy words are shown on
/// the sale screen, for one) is listed once, under the first name.
pub fn program_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    let lists = [
        openbmt_counter::say::all_words(),
        all_words(),
        openbmt_watch::say::all_words(),
        openbmt_core::say_policy::all_words(),
    ];
    for (name, words) in lists.into_iter().flatten() {
        if !all
            .iter()
            .any(|(_, w)| w.heading == words.heading && w.what == words.what)
        {
            all.push((name, words));
        }
    }
    all
}

fn who_text(who: Who) -> &'static str {
    match who {
        Who::Nobody => "nobody",
        Who::Customer => "the customer",
        Who::Cashier => "the cashier",
        Who::Manager => "a manager",
        Who::Owner => "the owner",
        Who::KeyHolder => "the key holder",
        Who::BoxKeeper => "the Box keeper",
    }
}

fn cell(text: &str) -> String {
    text.replace('|', "/")
}

/// The list of every word the counter program can show, as one Markdown
/// table (`docs/screens/counter.md`). Run the test `sc_12_the_list_on_disk_is_current`
/// with `OPENBMT_WRITE_SCREENS=1` to write it again.
pub fn screens_markdown() -> String {
    let mut out = String::new();
    out.push_str("# Counter: every screen\n\n");
    out.push_str("What the counter program tells the cashier, a manager and the owner\n");
    out.push_str("(`docs/SCREEN-RULES.md`). The list is made from the code; do not edit it\n");
    out.push_str("by hand. To write it again, run the test\n");
    out.push_str("`sc_12_the_list_on_disk_is_current` of `openbmt-counter-node` with\n");
    out.push_str("`OPENBMT_WRITE_SCREENS=1`.\n\n");
    out.push_str("| state | heading | what happened | what to do next | who | code |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for (name, w) in program_words() {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            cell(&name),
            cell(&w.heading),
            cell(&w.what),
            cell(&w.next),
            who_text(w.who),
            w.code.unwrap_or("")
        ));
    }
    out
}
