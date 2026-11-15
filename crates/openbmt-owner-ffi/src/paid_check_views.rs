//! The types of the counter checks (`docs/OWNER-COINS-RULES.md`, OC-60 to
//! OC-64): what the owner phone reads to show a counter's Paid that the
//! wallet does not show, and what it sends to clear one. The app writes the
//! words; these carry the facts.

use crate::codec::{wire_record, wire_record_get};

/// An alarm: a counter showed a sale paid and this phone does not see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidAlarmView {
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The sale's number.
    pub sale: u32,
    /// Who took the sale, as the counter recorded; empty when it names
    /// nobody.
    pub cashier: String,
    /// What the check expects, in satoshis: the most recorded or the amount
    /// last quoted.
    pub recorded_sats: u64,
    /// What this phone sees paid to the sale's address, in satoshis.
    pub seen_sats: u64,
    /// When this phone first read the Paid, by its clock.
    pub first_read: u64,
    /// When the alarm was raised, by this phone's clock.
    pub raised_at: u64,
    /// The cashier and the amounts are known. False when the sale is no
    /// longer read from the counter's records (its counter is held back, or
    /// its copy does not verify): they are then empty and zero, and the
    /// alarm shows only the counter, the sale and the times.
    pub known: bool,
}

wire_record!(PaidAlarmView {
    counter,
    sale,
    cashier,
    recorded_sats,
    seen_sats,
    first_read,
    raised_at,
    known
});

/// A sale whose payment arrived in full after an alarm was raised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidArrivedView {
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The sale's number.
    pub sale: u32,
    /// When it cleared, by this phone's clock.
    pub cleared_at: u64,
}

wire_record!(PaidArrivedView {
    counter,
    sale,
    cleared_at
});

/// A sale that is waiting for a block, or that cannot be checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidSaleLineView {
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The sale's number.
    pub sale: u32,
    /// What the check expects, in satoshis: the most recorded or the amount
    /// last quoted.
    pub recorded_sats: u64,
    /// When this phone first read the Paid, by its clock.
    pub first_read: u64,
}

wire_record!(PaidSaleLineView {
    counter,
    sale,
    recorded_sats,
    first_read
});

/// What `paid_check` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidCheckView {
    /// The checks keep their marks. False when the file of marks does not
    /// read back or could not be written: the check still runs, on marks held
    /// in memory, until `paid_check_reset`.
    pub kept: bool,
    /// There are more marks than a file keeps.
    pub full: bool,
    /// The counters with a record that does not read as part of a sale, 32
    /// bytes each: some of their payments cannot be checked.
    pub unreadable: Vec<Vec<u8>>,
    /// The alarms not cleared, oldest first.
    pub alarms: Vec<PaidAlarmView>,
    /// Alarms that cleared themselves lately.
    pub arrived: Vec<PaidArrivedView>,
    /// Sales whose payment is not in a block yet.
    pub waiting: Vec<PaidSaleLineView>,
    /// The newest sales too old for the wallet to check, at most 200.
    pub not_checkable: Vec<PaidSaleLineView>,
    /// How many more there are than `not_checkable` lists.
    pub not_checkable_more: u32,
    /// The newest sales that cannot be checked because the receipt log is
    /// full, at most 200.
    pub log_full: Vec<PaidSaleLineView>,
    /// How many more there are than `log_full` lists.
    pub log_full_more: u32,
}

wire_record!(PaidCheckView {
    kept,
    full,
    unreadable,
    alarms,
    arrived,
    waiting,
    not_checkable,
    not_checkable_more,
    log_full,
    log_full_more
});

/// The arguments of `paid_day`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidDayArgs {
    /// The time now, in seconds since 1970.
    pub now: u64,
    /// The zone's offset from UTC, in seconds: the day is from local midnight
    /// to the next.
    pub offset_secs: i32,
}

wire_record!(PaidDayArgs { now, offset_secs });
wire_record_get!(PaidDayArgs { now, offset_secs });

/// One counter's day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidDayView {
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// Sales paid on chain in the day.
    pub sales: u32,
    /// What the counter recorded as received over them, in satoshis.
    pub total_sats: u64,
    /// Seen in a block, in full.
    pub seen: u32,
    /// Waiting for a block.
    pub waiting: u32,
    /// Not checkable: too old, or the receipt log is full.
    pub too_old: u32,
    /// Missing.
    pub missing: u32,
    /// What this phone saw over them, in satoshis.
    pub seen_sats: u64,
    /// The total differs from what was seen.
    pub differs: bool,
}

wire_record!(PaidDayView {
    counter,
    sales,
    total_sats,
    seen,
    waiting,
    too_old,
    missing,
    seen_sats,
    differs
});

/// The arguments of `paid_clear`: the alarm as `paid_check` showed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidClearArgs {
    /// The time now, in seconds since 1970.
    pub now: u64,
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The sale's number.
    pub sale: u32,
}

wire_record!(PaidClearArgs { now, counter, sale });
wire_record_get!(PaidClearArgs { now, counter, sale });
