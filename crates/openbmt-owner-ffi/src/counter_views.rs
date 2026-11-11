//! The types of the ops that add a counter (`docs/SESSION-RULES.md`, AS-69
//! to AS-73): what the owner phone shows after it reads a counter's
//! request, what it asks to confirm, and the code the counter takes. They
//! are written and read like those of [`views`](crate::views).

use crate::codec::{wire_record, wire_record_get};
use crate::views::{CounterShareView, WordsView};

/// What `counter_scan` queued and what the owner is shown (AS-69, AS-70).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterScanView {
    /// The pairing's number in the queue.
    pub pairing: u32,
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The name the counter gave itself. It is text from the counter.
    pub name: String,
    /// The four words the counter and the Key app show.
    pub session_words: Vec<String>,
    /// The phone holds no settings yet: confirming needs today's rate in
    /// satoshis to a dollar.
    pub needs_rate: bool,
    /// How many seconds the counter waits for its code, from the time it
    /// began.
    pub window_secs: u32,
    /// Each counter's share of the instant limit once this one is added.
    pub shares: Vec<CounterShareView>,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(CounterScanView {
    pairing,
    counter,
    name,
    session_words,
    needs_rate,
    window_secs,
    shares,
    words
});

/// The arguments of `counter_confirm` after the time (AS-70, AS-71).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterConfirmArgs {
    /// The pairing's number, from `counter_scan`.
    pub pairing: u32,
    /// Satoshis to a dollar, for the starting settings when the phone holds
    /// none; ignored when it does.
    pub sats_per_usd: u64,
}

wire_record!(CounterConfirmArgs {
    pairing,
    sats_per_usd
});
wire_record_get!(CounterConfirmArgs {
    pairing,
    sats_per_usd
});

/// What `counter_confirm` queued (AS-70, AS-71).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterQueuedView {
    /// The settings' number in the queue.
    pub settings: u32,
    /// The settings' version.
    pub version: u32,
    /// The first batch's number in the queue.
    pub batch: u32,
    /// How many addresses the first batch holds.
    pub batch_count: u32,
    /// Each counter's share of the instant limit in the new settings.
    pub shares: Vec<CounterShareView>,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(CounterQueuedView {
    settings,
    version,
    batch,
    batch_count,
    shares,
    words
});

/// A counter whose finished pairing waits to be handed over (AS-73).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingCounterView {
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The name the counter gave itself. It is text from the counter.
    pub name: String,
}

wire_record!(WaitingCounterView { counter, name });

/// The code a counter takes (AS-72).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterBundleView {
    /// The counter bundle (`OBMs`), to show as a code.
    pub code: Vec<u8>,
    /// The first batch of addresses is in it.
    pub with_batch: bool,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(CounterBundleView {
    code,
    with_batch,
    words
});
