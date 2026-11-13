//! SS-35 to SS-38, SS-43: the limit and the date as pure functions. The
//! limit (in satoshis, as the caller worked it out from the shop's
//! currency), the date to move (seconds since the epoch) and the end of a
//! "Not yet" are held by the caller, the owner phone; nothing here reads a
//! clock or the signed settings.

/// SS-37: the longest a "Not yet" lasts, in seconds (7 days).
pub const MAX_NOT_YET_SECS: u64 = 7 * 24 * 60 * 60;

/// SS-37: where the starter wallet stands against its limit and its date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitState {
    /// Under the limit and before the date: nothing is asked.
    Under,
    /// Holding the limit or more: the owner phone opens on the upgrade
    /// screen.
    AtLimit,
    /// The date to move has come (and the wallet is under its limit).
    PastDate,
    /// At the limit or past the date, and "Not yet" is in force until
    /// `until`.
    NotYet {
        /// The time "Not yet" ends, seconds since the epoch.
        until: u64,
    },
}

/// SS-37: the time a "Not yet" granted at `now` ends: at most 7 days on,
/// and sooner when `asked_secs` is less.
pub fn not_yet_until(now: u64, asked_secs: u64) -> u64 {
    now.saturating_add(asked_secs.min(MAX_NOT_YET_SECS))
}

/// SS-35 to SS-37: the state of the starter wallet.
///
/// `held` is what it holds, counting what is on its way, and `limit` the
/// most it may hold, both in satoshis. A limit of 0 is a wallet with no
/// limit set and counts as at its limit. `move_by` is the date to move,
/// `now` the time, both in seconds. `not_yet_until` is the end of a "Not
/// yet" the owner took; it counts only while it is in the future and no
/// more than 7 days ahead, so a stored value that is further than the most
/// a "Not yet" lasts never holds anything off.
pub fn limit_state(
    held: u64,
    limit: u64,
    now: u64,
    move_by: u64,
    not_yet_until: Option<u64>,
) -> LimitState {
    let at_limit = held >= limit;
    let past_date = now >= move_by;
    if !at_limit && !past_date {
        return LimitState::Under;
    }
    if let Some(until) = not_yet_until {
        if until > now && until - now <= MAX_NOT_YET_SECS {
            return LimitState::NotYet { until };
        }
    }
    if at_limit {
        LimitState::AtLimit
    } else {
        LimitState::PastDate
    }
}

/// A change of the shop's settings the owner may ask for, as SS-37 sorts
/// them. The limit and the date are held by the phone, not in the signed
/// settings, so these are the owner phone's own screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsChange {
    /// Raising the limit.
    RaiseLimit,
    /// Moving the date later.
    LaterDate,
    /// Adding a counter.
    AddCounter,
    /// Turning "Stop bitcoin sales above my limit" off.
    StopAboveOff,
    /// Any other change to the settings.
    Other,
    /// Lowering the limit.
    LowerLimit,
    /// Moving the date earlier.
    EarlierDate,
    /// Turning "Stop bitcoin sales above my limit" on.
    StopAboveOn,
}

impl SettingsChange {
    /// Whether the change leaves the wallet holding more, for longer, or
    /// the shop more open than before. Only the changes that make the
    /// wallet safer are left out.
    fn loosens(self) -> bool {
        !matches!(
            self,
            Self::LowerLimit | Self::EarlierDate | Self::StopAboveOn
        )
    }
}

/// SS-37: whether a change waits. While "Not yet" lasts, raising the limit,
/// adding counters and other changes to settings wait; so they do while the
/// wallet is at its limit or past its date and the owner has taken neither
/// the upgrade nor "Not yet". Only the changes that tighten (a lower limit,
/// an earlier date, "stop above my limit" on) go through, and everything
/// goes through while the wallet is under its limit and before its date.
/// Switching a counter to the new wallet (SS-41) is the upgrade, not a
/// change of this kind, and is never held.
pub fn change_held(state: LimitState, change: SettingsChange) -> bool {
    state != LimitState::Under && change.loosens()
}

/// SS-38: whether a counter may offer bitcoin. Always, unless the owner
/// turned on "Stop bitcoin sales above my limit", and then only while the
/// wallet holds less than the limit.
pub fn counter_offers_bitcoin(stop_above_limit: bool, held: u64, limit: u64) -> bool {
    !stop_above_limit || held < limit
}

/// SS-43: the sealed seed and the Keystore key may be deleted when the move
/// of SS-42 has one confirmation.
pub fn seed_may_go(move_confirmations: u32) -> bool {
    move_confirmations >= 1
}
