//! The Key app's own screens: the allowlist, the ceilings and the marks.
//!
//! Each call is one call of the signer's public API, made through
//! `Approver::change_signer` (or `raise_receive_mark`), which stores the
//! new state before the Key app takes it. Nothing here chooses a value or
//! checks a bound.

use std::convert::Infallible;
use std::str::FromStr;
use std::sync::Arc;

use bitcoin::Address;
use openbmt_approver::StateError;
use openbmt_signer::{
    Ceilings, FeeCeilings, SignerState, ALLOWLIST_WAIT_SECS, MAX_FEE_RATE_SAT_PER_VB, MAX_FEE_SATS,
};

use crate::app::{save_ready, Inner, KeyApp, ReadyState};
use crate::error::{refused, refused_by, store_failed, KeyError, StoreError};
use crate::views::script_hex;

/// Three ceilings in satoshis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct CeilingsView {
    /// Most one transaction may send.
    pub per_tx: u64,
    /// Most that may be sent in any 24 hours.
    pub per_day: u64,
    /// Most that may be sent in any 7 days.
    pub per_week: u64,
}

impl From<Ceilings> for CeilingsView {
    fn from(c: Ceilings) -> Self {
        Self {
            per_tx: c.per_tx,
            per_day: c.per_day,
            per_week: c.per_week,
        }
    }
}

impl From<CeilingsView> for Ceilings {
    fn from(c: CeilingsView) -> Self {
        Self {
            per_tx: c.per_tx,
            per_day: c.per_day,
            per_week: c.per_week,
        }
    }
}

/// The two fee ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct FeeCeilingsView {
    /// Highest fee rate, in satoshis per virtual byte.
    pub sat_per_vb: u64,
    /// Highest fee of one transaction, in satoshis.
    pub absolute_sats: u64,
}

/// A cash-out destination.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AllowlistView {
    /// The address, in the wallet's network.
    pub address: Option<String>,
    /// The script, as hex.
    pub script_hex: String,
    /// The label typed on this device.
    pub label: String,
    /// The time it was enrolled, in seconds.
    pub enrolled_at: u64,
    /// The time from which it counts as allowlisted; `None` if never.
    pub active_at: Option<u64>,
}

/// What the signer holds, for the Key app's own screen.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SignerStatus {
    /// Ceilings for other destinations (refunds).
    pub ceilings_other: CeilingsView,
    /// Ceilings for allowlisted destinations.
    pub ceilings_allowlisted: CeilingsView,
    /// The fee ceilings.
    pub fee_ceilings: FeeCeilingsView,
    /// The highest fee rate the Key app lets anyone set, in sat/vB.
    pub max_fee_rate_sat_per_vb: u64,
    /// The highest absolute fee ceiling the Key app lets anyone set.
    pub max_fee_sats: u64,
    /// Seconds a new destination waits before it is allowlisted.
    pub allowlist_wait_secs: u64,
    /// The receive mark.
    pub receive_mark: u32,
    /// The change mark.
    pub change_mark: u32,
    /// The latest time seen, in seconds.
    pub latest_time_seen: u64,
    /// The time the rules use now: the larger of the clock and the latest
    /// time seen.
    pub effective_time_now: u64,
    /// Other destinations plus fees in the last 24 hours.
    pub other_day: u64,
    /// Other destinations plus fees in the last 7 days.
    pub other_week: u64,
    /// Allowlisted destinations in the last 24 hours.
    pub allowlisted_day: u64,
    /// Allowlisted destinations in the last 7 days.
    pub allowlisted_week: u64,
    /// How many transactions the spend log holds.
    pub spend_log_count: u32,
    /// The destinations.
    pub allowlist: Vec<AllowlistView>,
}

fn state_error(e: StateError<StoreError>) -> KeyError {
    match e {
        StateError::Store(why) => store_failed(&why),
        other => refused(other),
    }
}

/// An address the person scanned, in the wallet's network.
fn script_of(address: &str, inner: &mut Inner) -> Result<bitcoin::ScriptBuf, KeyError> {
    let network = inner.ready()?.wallet.network;
    let parsed = Address::from_str(address.trim())
        .map_err(|e| refused_by("address", format!("not an address: {e}")))?;
    let checked = parsed.require_network(network).map_err(|e| {
        refused_by(
            "address",
            format!("the address is for another network: {e}"),
        )
    })?;
    Ok(checked.script_pubkey())
}

/// One change of the signer's state, stored before the Key app takes it.
fn change_signer(
    inner: &mut Inner,
    f: impl FnOnce(&mut SignerState) -> Result<(), String>,
) -> Result<(), KeyError> {
    inner.no_session()?;
    let store = Arc::clone(&inner.store);
    let ReadyState {
        material,
        wallet,
        approver,
    } = inner.ready()?;
    approver
        .change_signer(|snapshot| save_ready(&store, material, wallet, snapshot), f)
        .map_err(state_error)
}

fn no_store<T>(r: Result<T, openbmt_signer::ChangeError<Infallible>>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

#[uniffi::export]
impl KeyApp {
    /// What the signer holds. Refused when key 2 is not on this device.
    pub fn signer_status(&self) -> Result<SignerStatus, KeyError> {
        self.run(|inner| {
            let now = inner.store.now();
            let ready = inner.ready()?;
            let network = ready.wallet.network;
            let signer = ready
                .approver
                .signer()
                .ok_or_else(|| refused_by("AS-36", "AS-36: key 2 is not on this device"))?;
            let totals = signer.totals(now);
            let fee = signer.fee_ceilings();
            let allowlist = signer
                .allowlist()
                .map(|e| AllowlistView {
                    address: Address::from_script(e.script(), network)
                        .ok()
                        .map(|a| a.to_string()),
                    script_hex: script_hex(e.script().as_bytes()),
                    label: e.label().to_string(),
                    enrolled_at: e.enrolled_at(),
                    active_at: e.active_at(),
                })
                .collect();
            Ok(SignerStatus {
                ceilings_other: signer.ceilings_other().into(),
                ceilings_allowlisted: signer.ceilings_allowlisted().into(),
                fee_ceilings: FeeCeilingsView {
                    sat_per_vb: fee.sat_per_vb,
                    absolute_sats: fee.absolute_sats,
                },
                max_fee_rate_sat_per_vb: MAX_FEE_RATE_SAT_PER_VB,
                max_fee_sats: MAX_FEE_SATS,
                allowlist_wait_secs: ALLOWLIST_WAIT_SECS,
                receive_mark: signer.receive_mark(),
                change_mark: signer.change_mark(),
                latest_time_seen: signer.latest_time_seen(),
                effective_time_now: signer.effective_time(now),
                other_day: totals.other_day,
                other_week: totals.other_week,
                allowlisted_day: totals.allowlisted_day,
                allowlisted_week: totals.allowlisted_week,
                spend_log_count: u32::try_from(signer.log().count()).unwrap_or(u32::MAX),
                allowlist,
            })
        })
    }

    /// The receive mark, which every Key app has: the signer's when key 2
    /// is on this device.
    pub fn receive_mark(&self) -> Result<u32, KeyError> {
        self.run(|inner| Ok(inner.ready()?.approver.receive_mark()))
    }

    /// SR-30: enroll a cash-out destination from the address the Key app's
    /// own camera scanned, with a label typed on this device. It counts as
    /// allowlisted after the wait.
    pub fn enroll_destination(&self, address: String, label: String) -> Result<(), KeyError> {
        self.run(|inner| {
            let now = inner.store.now();
            let script = script_of(&address, inner)?;
            change_signer(inner, |s| {
                no_store(s.enroll_destination(script, &label, now, |_| Ok::<(), Infallible>(())))
            })
        })
    }

    /// Remove a destination from the allowlist. It takes effect at once.
    pub fn remove_destination(&self, address: String) -> Result<(), KeyError> {
        self.run(|inner| {
            let script = script_of(&address, inner)?;
            change_signer(inner, |s| {
                no_store(s.remove_destination(&script, |_| Ok::<(), Infallible>(())))
            })
        })
    }

    /// SR-37: set the six ceilings.
    pub fn set_ceilings(
        &self,
        other: CeilingsView,
        allowlisted: CeilingsView,
    ) -> Result<(), KeyError> {
        self.run(|inner| {
            change_signer(inner, |s| {
                no_store(s.set_ceilings(other.into(), allowlisted.into(), |_| {
                    Ok::<(), Infallible>(())
                }))
            })
        })
    }

    /// SR-40 to SR-42: set the two fee ceilings.
    pub fn set_fee_ceilings(&self, fee: FeeCeilingsView) -> Result<(), KeyError> {
        self.run(|inner| {
            change_signer(inner, |s| {
                no_store(s.set_fee_ceilings(
                    FeeCeilings {
                        sat_per_vb: fee.sat_per_vb,
                        absolute_sats: fee.absolute_sats,
                    },
                    |_| Ok::<(), Infallible>(()),
                ))
            })
        })
    }

    /// SR-34: raise the change mark. It only goes up.
    pub fn raise_change_mark(&self, new_mark: u32) -> Result<(), KeyError> {
        self.run(|inner| {
            change_signer(inner, |s| {
                no_store(s.raise_change_mark(new_mark, |_| Ok::<(), Infallible>(())))
            })
        })
    }

    /// SR-70: raise the receive mark, on this device only. It only goes up.
    pub fn raise_receive_mark(&self, new_mark: u32) -> Result<(), KeyError> {
        self.run(|inner| {
            inner.no_session()?;
            let store = Arc::clone(&inner.store);
            let ReadyState {
                material,
                wallet,
                approver,
            } = inner.ready()?;
            approver
                .raise_receive_mark(new_mark, |snapshot| {
                    save_ready(&store, material, wallet, snapshot)
                })
                .map_err(state_error)
        })
    }

    /// SR-31: confirm a clock correction, using the clock the app gave.
    pub fn correct_clock(&self) -> Result<(), KeyError> {
        self.run(|inner| {
            let now = inner.store.now();
            change_signer(inner, |s| {
                no_store(s.correct_clock(now, |_| Ok::<(), Infallible>(())))
            })
        })
    }
}
