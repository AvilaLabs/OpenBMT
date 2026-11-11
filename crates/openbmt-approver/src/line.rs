//! What the Key app shows for each item (AS-23 to AS-29).

use openbmt_core::identity::DeviceId;
use openbmt_core::settings::Change;
use openbmt_session::{ItemKind, Label};
use openbmt_signer::{OutputKind, Remaining};

/// A decision on one line (AS-30).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Approve the line.
    Approve,
    /// Decline the line, with a reason of 1 to 160 bytes.
    Decline(String),
}

/// One output of a transaction, as the signer's review found it, next to
/// the owner phone's label (AS-24).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    /// Position in the transaction.
    pub index: usize,
    /// The address, in the wallet's network, if the script has one.
    pub address: Option<String>,
    /// The output's script, as hex.
    pub script_hex: String,
    /// The amount in satoshis.
    pub amount_sats: u64,
    /// What the signer found: change, allowlisted (with the Key app's own
    /// label) or another destination.
    pub kind: OutputKind,
    /// The owner phone's label, which proves nothing. `None` when the owner
    /// phone gave this output no label.
    pub label_from_owner_phone: Option<Label>,
}

/// What the signer's review found out about a transaction (AS-24).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendReview {
    /// Every output, in transaction order, change included.
    pub outputs: Vec<OutputLine>,
    /// The change index and amount, only if the signer proved it.
    pub change: Option<(u32, u64)>,
    /// The only output is change: coins move inside the wallet.
    pub moves_coins_inside_wallet: bool,
    /// The fee in satoshis.
    pub fee_sats: u64,
    /// The fee rate in satoshis per virtual byte, rounded up.
    pub fee_rate_sat_per_vb: u64,
    /// The number of inputs.
    pub input_count: usize,
    /// The total of the inputs in satoshis.
    pub input_total_sats: u64,
    /// The total to other destinations.
    pub other_total_sats: u64,
    /// The total to allowlisted destinations.
    pub allowlisted_total_sats: u64,
    /// What would remain under each ceiling.
    pub remaining: Remaining,
    /// Whether the transaction is already in the spend log (counts once).
    pub already_counted: bool,
    /// How far the clock is behind the latest time seen, if it is.
    pub clock_behind_by: Option<u64>,
}

/// The line of a `Spend` item (AS-24, AS-25).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendLine {
    /// The review, or `None` when the signer refused the file or this
    /// device does not hold key 2.
    pub review: Option<SpendReview>,
    /// Outputs that have no label from the owner phone and are not change.
    pub outputs_without_label: Vec<usize>,
    /// Labels that name an output the transaction does not have (AS-25).
    pub labels_without_output: Vec<u32>,
    /// Outputs the owner phone labelled change that the signer did not
    /// prove to be change.
    pub claimed_change_not_proven: Vec<usize>,
}

/// Whether a check passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// The check passed.
    Passed,
    /// The check failed.
    Failed,
    /// An earlier check failed, so this one did not run.
    NotChecked,
}

/// One of the checks of `docs/DESIGN.md` §5.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckStep {
    /// What the check is.
    pub name: &'static str,
    /// Whether it passed.
    pub status: StepStatus,
}

/// The line of a `BatchRefill` item (AS-26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchLine {
    /// The counter the batch is for.
    pub counter: DeviceId,
    /// The first index.
    pub start: u32,
    /// One past the last index, if the range is in bounds.
    pub end: Option<u32>,
    /// The number of addresses.
    pub count: u32,
    /// The receive mark the range must start at.
    pub receive_mark: u32,
    /// The checks, in their order.
    pub checks: Vec<CheckStep>,
}

/// One field of the settings that differs (AS-27).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingChange {
    /// The field's name.
    pub field: &'static str,
    /// The value in the last settings the Key app co-signed, or `None` when
    /// it co-signed none.
    pub old: Option<String>,
    /// The new value.
    pub new: String,
    /// Whether this field loosens or tightens.
    pub direction: Change,
}

/// The line of a `Settings` item (AS-27).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsLine {
    /// The new settings' version.
    pub new_version: u32,
    /// The version of the last settings the Key app co-signed.
    pub last_version: Option<u32>,
    /// Every field that differs, or every field when the Key app co-signed
    /// none before.
    pub changes: Vec<SettingChange>,
    /// Whether the change as a whole loosens, `None` if the checks failed.
    pub overall: Option<Change>,
    /// The checks: owner signature, values, version.
    pub checks: Vec<CheckStep>,
}

/// The line of a `Pairing` item (AS-28).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingLine {
    /// The counter's name.
    pub name: String,
    /// The counter's key, in full.
    pub counter: DeviceId,
    /// The four session words, when the request passed its checks. They
    /// take a moment to work out.
    pub session_words: Option<[&'static str; 4]>,
    /// The settings version the owner phone claims is in force.
    pub claimed_settings_version: u32,
    /// The settings version the Key app would sign.
    pub settings_version_to_sign: u32,
}

/// The line of a `BoxClaim` item (AS-81).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxClaimLine {
    /// The Box's key, in full.
    pub box_key: DeviceId,
    /// The four fingerprint words of the Box's key, the words the Box's
    /// console and status page show when it is paired. They take a moment to
    /// work out.
    pub words: Option<[&'static str; 4]>,
    /// The checks, in their order.
    pub checks: Vec<CheckStep>,
}

/// The line of a `HealthCheck` item (AS-29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthLine {
    /// The number the Key app would sign.
    pub nonce: [u8; 32],
}

/// What a line shows, by kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail {
    /// A transaction.
    Spend(SpendLine),
    /// An address batch.
    Batch(BatchLine),
    /// Settings.
    Settings(SettingsLine),
    /// A pairing.
    Pairing(PairingLine),
    /// A health check.
    Health(HealthLine),
    /// A Box's claim.
    BoxClaim(BoxClaimLine),
}

/// One line of the session, made from the Key app's own checks (AS-23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The item's number.
    pub item: u32,
    /// The item's kind.
    pub kind: ItemKind,
    /// What the checks found.
    pub detail: Detail,
    /// Whether the line can be approved (AS-23).
    pub can_approve: bool,
    /// When it cannot: the failing rule, in words.
    pub failure: Option<String>,
    /// The decision made so far (AS-30).
    pub decision: Option<Choice>,
}
