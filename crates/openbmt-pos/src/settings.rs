//! What the shop decides about its POS (PS-10, PS-13, PS-14) and the timing
//! of the matching (every figure **[to tune]**).

use std::collections::BTreeSet;
use std::fmt;

use openbmt_core::identity::DeviceId;
use openbmt_counter::Currency;

use crate::model::{PosTender, TenderKind, Text};
use crate::{
    CLOSED_BEFORE_PAID_SECS, GRACE_SECS, MATCH_WINDOW_SECS, MAX_COUNTERS_PER_LOCATION,
    MAX_CURRENCIES, MAX_LOCATIONS, MAX_NO_POS, MAX_TENDER_NAMES, NEVER_PAID_SPAN_SECS, RETIRE_SECS,
};

/// Why settings were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    /// PS-10: no Bitcoin tender name, too many, or a name of only spaces.
    TenderNames,
    /// PS-13: no currency, too many, or one listed twice.
    Currencies,
    /// PS-14: too many locations, a location listed twice, too many counters
    /// at one, or a counter in two places.
    Locations,
    /// A timing figure is zero, or the retire time is not above the grace.
    Timing,
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TenderNames => write!(
                f,
                "PS-10: the settings need 1 to {MAX_TENDER_NAMES} Bitcoin tender names, none blank"
            ),
            Self::Currencies => write!(
                f,
                "PS-13: the settings need 1 to {MAX_CURRENCIES} different currencies"
            ),
            Self::Locations => write!(
                f,
                "PS-14: locations are unique, with each counter in at most one place"
            ),
            Self::Timing => write!(f, "a timing figure is zero or out of order"),
        }
    }
}

impl std::error::Error for SettingsError {}

/// The counters that serve one POS location (PS-14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationCounters {
    /// The location, exactly as the POS names it.
    pub location: Text,
    /// The counters at it.
    pub counters: Vec<DeviceId>,
}

/// The timing of the matching, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// PS-40, PS-42, PS-44: how long a tender or a paid sale waits for the
    /// other side.
    pub grace_secs: u64,
    /// PS-41: how far before the paid time a tender may be.
    pub closed_before_paid_secs: u64,
    /// PS-33: how long after first reported paid a tender may match.
    pub match_window_secs: u64,
    /// PS-33: the stand-in span after opening for a sale never reported
    /// paid.
    pub never_paid_span_secs: u64,
    /// PS-73: how long an order is unchanged before it can retire.
    pub retire_secs: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            grace_secs: GRACE_SECS,
            closed_before_paid_secs: CLOSED_BEFORE_PAID_SECS,
            match_window_secs: MATCH_WINDOW_SECS,
            never_paid_span_secs: NEVER_PAID_SPAN_SECS,
            retire_secs: RETIRE_SECS,
        }
    }
}

/// The shop's settings for the POS side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// PS-10: the exact names of the shop's Bitcoin tender.
    pub bitcoin_tenders: Vec<Text>,
    /// PS-14: location to counters.
    pub locations: Vec<LocationCounters>,
    /// PS-14: counters used without a POS.
    pub no_pos: Vec<DeviceId>,
    /// PS-13: the shop's currencies.
    pub currencies: Vec<Currency>,
    /// The timing.
    pub timing: Timing,
}

impl Settings {
    /// Settings with the default timing and no counter in no-POS mode.
    pub fn new(
        bitcoin_tenders: Vec<Text>,
        locations: Vec<LocationCounters>,
        currencies: Vec<Currency>,
    ) -> Result<Self, SettingsError> {
        let s = Self {
            bitcoin_tenders,
            locations,
            no_pos: Vec::new(),
            currencies,
            timing: Timing::default(),
        };
        s.validate()?;
        Ok(s)
    }

    /// Check every bound.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.bitcoin_tenders.is_empty()
            || self.bitcoin_tenders.len() > MAX_TENDER_NAMES
            || self
                .bitcoin_tenders
                .iter()
                .any(|n| n.as_str().trim_ascii().is_empty())
        {
            return Err(SettingsError::TenderNames);
        }
        let currencies: BTreeSet<_> = self.currencies.iter().collect();
        if self.currencies.is_empty()
            || self.currencies.len() > MAX_CURRENCIES
            || currencies.len() != self.currencies.len()
        {
            return Err(SettingsError::Currencies);
        }
        let mut seen_locations = BTreeSet::new();
        let mut seen_counters = BTreeSet::new();
        if self.locations.len() > MAX_LOCATIONS || self.no_pos.len() > MAX_NO_POS {
            return Err(SettingsError::Locations);
        }
        for l in &self.locations {
            if l.counters.len() > MAX_COUNTERS_PER_LOCATION || !seen_locations.insert(&l.location) {
                return Err(SettingsError::Locations);
            }
            for c in &l.counters {
                if !seen_counters.insert(c) {
                    return Err(SettingsError::Locations);
                }
            }
        }
        for c in &self.no_pos {
            if !seen_counters.insert(c) {
                return Err(SettingsError::Locations);
            }
        }
        let t = &self.timing;
        let zero = [
            t.grace_secs,
            t.closed_before_paid_secs,
            t.match_window_secs,
            t.never_paid_span_secs,
            t.retire_secs,
        ]
        .contains(&0);
        if zero || t.retire_secs <= t.grace_secs {
            return Err(SettingsError::Timing);
        }
        Ok(())
    }

    /// PS-10, PS-11: whether a tender counts as a Bitcoin tender. Its kind is
    /// `Bitcoin`, or its name equals a configured name without regard to
    /// ASCII case and surrounding spaces. Nothing is compared more loosely.
    pub fn is_bitcoin(&self, tender: &PosTender) -> bool {
        tender.kind == TenderKind::Bitcoin
            || self.bitcoin_tenders.iter().any(|n| {
                n.as_str()
                    .trim_ascii()
                    .eq_ignore_ascii_case(tender.name.as_str().trim_ascii())
            })
    }

    /// PS-14: the counters of a location; empty for one the settings do not
    /// name.
    pub fn counters_at(&self, location: &Text) -> &[DeviceId] {
        self.locations
            .iter()
            .find(|l| &l.location == location)
            .map_or(&[], |l| l.counters.as_slice())
    }

    /// PS-14: whether a counter is in no-POS mode.
    pub fn is_no_pos(&self, counter: &DeviceId) -> bool {
        self.no_pos.contains(counter)
    }
}
