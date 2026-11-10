//! Refunds: the cashier's side (CR-60 to CR-74).
//!
//! A refund request is a record, not a payment (CR-60). This module holds
//! what a request is made of: the destination read from the customer's QR
//! code ([`refund_destination`], CR-62, CR-63), how the amount is given
//! ([`RefundAsk`]), the entries kept with each sale ([`RefundEntry`], stored
//! inside `OBML` version 2) and what the cashier's screen is told
//! ([`RefundView`]). The calls themselves are `Counter::request_refund`,
//! `Counter::withdraw_refund` and `Counter::refund_in_cash`.

use std::str::FromStr;

use bitcoin::{Address, Network, Script, ScriptBuf};
use openbmt_core::wire::{DecodeError, Reader};

use crate::counter::Actor;
use crate::desk::Role;
use crate::money::{Amount, Rate, RateSource, MAX_SATS, SATS_PER_BTC};
use crate::refusal::DestinationRefused;

/// CR-61: the longest destination script, in bytes (a P2WSH or P2TR script).
pub const MAX_SCRIPT_BYTES: usize = 34;
/// CR-67: most refund entries one sale keeps, requests and cash refunds
/// together.
pub const MAX_REFUNDS_PER_SALE: usize = 32;
/// CR-62: the longest text that is read as a refund QR code, in bytes.
pub const MAX_QR_BYTES: usize = 2_048;

/// CR-61: a name or a reason as the labels of AS-07 hold it: 1 to 64
/// characters, at most 255 bytes, no control character.
pub fn refund_text_ok(s: &str) -> bool {
    let chars = s.chars().count();
    (1..=64).contains(&chars) && s.len() <= 255 && !s.chars().any(char::is_control)
}

/// CR-63: whether `script` is one of the kinds the Key app's signer pays
/// (`docs/SIGNER-RULES.md` SR-22). The counter crate does not depend on the
/// signer, so this is a copy of the list; a test in a crate that depends on
/// both checks the two agree.
pub fn is_payment_script(script: &Script) -> bool {
    script.is_p2wpkh()
        || script.is_p2wsh()
        || script.is_p2tr()
        || script.is_p2pkh()
        || script.is_p2sh()
}

/// CR-67: the dust limit of a destination's kind in satoshis, at the default
/// dust relay rate, as the owner's queue holds it (AS-51). `None` for a kind
/// that is not paid. A copy of `openbmt_spend::dust_limit`; a test in a crate
/// that depends on both checks the two agree.
pub fn dust_limit(script: &Script) -> Option<u64> {
    if script.is_p2wpkh() {
        Some(294)
    } else if script.is_p2wsh() || script.is_p2tr() {
        Some(330)
    } else if script.is_p2pkh() {
        Some(546)
    } else if script.is_p2sh() {
        Some(540)
    } else {
        None
    }
}

/// CR-62, CR-63: the destination script of a refund, from the text a camera
/// read: a bare address or a `bitcoin:` URI.
///
/// Only the address is taken. In a URI everything after the first `?` is
/// ignored: the amount, the label, the message, and any other field,
/// including one named `req-`. This differs from the Key app's deposit
/// reader on purpose: that reader refuses a code with a `req-` field, as
/// BIP21 says, because it enrols an address; here the amount is the shop's
/// and the code may carry anything.
///
/// Refused: a text over [`MAX_QR_BYTES`] or with a character that is not
/// printable ASCII (CR-62), an empty or unreadable address (CR-62), an
/// address for another network and a script the signer would not pay
/// (CR-63).
pub fn refund_destination(text: &str, network: Network) -> Result<ScriptBuf, DestinationRefused> {
    if text.len() > MAX_QR_BYTES {
        return Err(DestinationRefused::TooLong);
    }
    let text = text.trim_ascii();
    if !text.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(DestinationRefused::NotPrintable);
    }
    let has_scheme = text
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("bitcoin:"));
    let address = match text.get(8..).filter(|_| has_scheme) {
        Some(rest) => rest.split('?').next().unwrap_or(""),
        None => text,
    };
    let parsed = Address::from_str(address).map_err(|_| DestinationRefused::NotAnAddress)?;
    let checked = parsed
        .require_network(network)
        .map_err(|_| DestinationRefused::WrongNetwork)?;
    let script = checked.script_pubkey();
    if !is_payment_script(&script) {
        return Err(DestinationRefused::ScriptKind);
    }
    Ok(script)
}

/// CR-61: how the caller gives the amount of a request. The shop's setting
/// "refund at the dollar price or at the bitcoin amount" is not in the signed
/// settings, so the caller passes the choice with each call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefundAsk {
    /// At the bitcoin amount: this many satoshis.
    Sats(u64),
    /// At the dollar price: an amount in the shop's currency, turned into
    /// satoshis at this rate, rounded down. The rate comes with its source
    /// and must be as fresh as CR-58 asks.
    Value {
        /// The amount in minor units.
        minor: u64,
        /// The rate, in minor units per bitcoin.
        rate: Rate,
        /// Where the rate came from (CR-57).
        source: RateSource,
    },
}

/// CR-61: how the amount of a request was given, as the record keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// In satoshis.
    Sats,
    /// In the shop's currency, at this rate.
    Value {
        /// The amount in minor units.
        minor: u64,
        /// The rate, in minor units per bitcoin.
        minor_per_btc: u64,
        /// Where the rate came from (CR-57).
        source: RateSource,
    },
}

/// What a request is made of (CR-61).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundRequest {
    /// The amount.
    pub ask: RefundAsk,
    /// Where the money goes, as [`refund_destination`] gave it.
    pub destination: ScriptBuf,
    /// Who asks. The caller has already checked the PIN.
    pub actor: Actor,
    /// Why.
    pub reason: String,
}

/// What a cash refund from the till is made of (CR-71).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashRefund {
    /// Who gives the cash. The caller has already checked the PIN.
    pub actor: Actor,
    /// The amount in the shop's currency, in minor units.
    pub minor: Amount,
    /// Why.
    pub reason: String,
}

/// CR-61: satoshis for an amount in the shop's currency at a rate, rounded
/// down: `floor(minor * 100_000_000 / minor_per_btc)`. `None` above all the
/// bitcoin there is.
pub(crate) fn sats_floor(minor: u64, rate: Rate) -> Option<u64> {
    let numerator = u128::from(minor).checked_mul(u128::from(SATS_PER_BTC))?;
    let sats = u64::try_from(numerator.checked_div(u128::from(rate.minor_per_btc()))?).ok()?;
    (sats <= MAX_SATS).then_some(sats)
}

/// What kind of entry a sale keeps (CR-72).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    /// A request that went to the owner (CR-60).
    Request,
    /// A refund paid in cash from the till (CR-71).
    Cash,
}

/// One refund entry of a sale, stored with it (`OBML` version 2).
///
/// ```text
/// kind u8 (1 request, 2 cash) | number u64 (0 for cash) | at u64
///   | role u8 | name_len u8 | name | sats u64
///   | value (flag, u64: minor units) | withdrawn flag
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RefundEntry {
    pub kind: EntryKind,
    pub number: u64,
    pub at: u64,
    pub role: Role,
    pub name: String,
    /// What was asked, in satoshis (a cash refund at the sale's last quote).
    pub sats: u64,
    /// The amount in the shop's currency, when it was given that way.
    pub value_minor: Option<u64>,
    pub withdrawn: bool,
}

impl RefundEntry {
    /// Whether the entry counts toward "never more than received" (CR-67).
    pub fn counts_as_asked(&self) -> bool {
        !self.withdrawn
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.push(match self.kind {
            EntryKind::Request => 1,
            EntryKind::Cash => 2,
        });
        out.extend_from_slice(&self.number.to_le_bytes());
        out.extend_from_slice(&self.at.to_le_bytes());
        out.push(self.role as u8);
        out.push(u8::try_from(self.name.len()).unwrap_or(u8::MAX));
        out.extend_from_slice(self.name.as_bytes());
        out.extend_from_slice(&self.sats.to_le_bytes());
        openbmt_core::wire::put_opt_u64(out, self.value_minor);
        out.push(u8::from(self.withdrawn));
    }

    pub fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let bad = DecodeError::NonCanonical;
        let kind = match r.u8()? {
            1 => EntryKind::Request,
            2 => EntryKind::Cash,
            _ => return Err(bad("a refund entry kind is not known")),
        };
        let number = r.u64_le()?;
        let at = r.u64_le()?;
        let role = Role::from_u8(r.u8()?).ok_or(bad("a role is not known"))?;
        let len = usize::from(r.u8()?);
        let name = std::str::from_utf8(r.take(len)?)
            .map_err(|_| bad("a name is not text"))?
            .to_string();
        if !refund_text_ok(&name) {
            return Err(bad("a name is not a label"));
        }
        let sats = r.u64_le()?;
        let value_minor = r.opt_u64()?;
        let withdrawn = r.flag()?;
        if sats == 0 || value_minor == Some(0) {
            return Err(bad("a refund entry is for nothing"));
        }
        match kind {
            EntryKind::Request if number == 0 => {
                return Err(bad("a request number is zero"));
            }
            EntryKind::Cash if number != 0 || withdrawn || value_minor.is_none() => {
                return Err(bad(
                    "a cash refund has a number, was withdrawn or has no amount",
                ));
            }
            _ => {}
        }
        Ok(Self {
            kind,
            number,
            at,
            role,
            name,
            sats,
            value_minor,
            withdrawn,
        })
    }
}

/// The state of a refund entry as the cashier's screen shows it (CR-72).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundState {
    /// Asked for; the sale is at the refund depth and the request is with
    /// the owner. The counter does not learn whether it was paid (CR-74).
    Requested,
    /// Asked for; the sale has not reached the refund depth, so the refund
    /// is not paid yet (CR-66).
    WaitingForDepth,
    /// The request was withdrawn (CR-70).
    Withdrawn,
    /// Paid in cash from the till (CR-71).
    PaidInCash,
}

impl RefundState {
    /// The words for the state.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Requested => "Refund requested",
            Self::WaitingForDepth => "Refund waiting for depth",
            Self::Withdrawn => "Refund withdrawn",
            Self::PaidInCash => "Refunded in cash",
        }
    }
}

/// One refund of a sale, for the screen and the receipt (CR-72). The words
/// on the customer's receipt are the app's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundView {
    /// The sale's receive index.
    pub sale: u32,
    /// The request number; none for a cash refund.
    pub number: Option<u64>,
    /// What was asked, in satoshis.
    pub asked: u64,
    /// The amount in the shop's currency, in minor units, when it was given
    /// that way (always for a cash refund).
    pub value_minor: Option<u64>,
    /// The state.
    pub state: RefundState,
    /// Who asked.
    pub by: Actor,
    /// When.
    pub at: u64,
}
