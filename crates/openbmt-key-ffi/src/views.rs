//! Flat, UI-ready records for the lines of a session. Every value is the
//! approver's own check; nothing is decided here.

use openbmt_approver::{
    BatchLine, CheckStep, Choice, Detail, HealthLine, Line, OutputLine, PairingLine, SettingChange,
    SettingsLine, SpendLine, SpendReview, StepStatus,
};
use openbmt_core::settings::Change;
use openbmt_session::{ItemKind, Label};
use openbmt_signer::{OutputKind, Remaining};

/// What kind of item a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// A transaction.
    Spend,
    /// An address batch.
    BatchRefill,
    /// Settings.
    Settings,
    /// A pairing.
    Pairing,
    /// A health check.
    HealthCheck,
}

/// The person's decision on a line (AS-30).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Approve the line.
    Approve,
    /// Decline it, with a reason of 1 to 160 bytes.
    Decline {
        /// The reason.
        reason: String,
    },
}

impl From<Decision> for Choice {
    fn from(d: Decision) -> Self {
        match d {
            Decision::Approve => Choice::Approve,
            Decision::Decline { reason } => Choice::Decline(reason),
        }
    }
}

impl From<&Choice> for Decision {
    fn from(c: &Choice) -> Self {
        match c {
            Choice::Approve => Decision::Approve,
            Choice::Decline(reason) => Decision::Decline {
                reason: reason.clone(),
            },
        }
    }
}

/// Whether a check passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// It passed.
    Passed,
    /// It failed.
    Failed,
    /// An earlier check failed, so this one did not run.
    NotChecked,
}

/// One check, in its order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckView {
    /// What the check is.
    pub name: String,
    /// What it found.
    pub status: CheckStatus,
}

/// Whether a change loosens or tightens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Stricter only.
    Tightening,
    /// Looser or mixed.
    Loosening,
}

/// What an output is, by the signer's own check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputKindView {
    /// Change back to the wallet, proven by the signer.
    Change {
        /// The change index.
        index: u32,
    },
    /// A destination on the Key app's allowlist.
    Allowlisted {
        /// The Key app's own label for it.
        label: String,
    },
    /// Any other destination.
    Other,
}

/// What an owner-phone label says an output is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerLabelKind {
    /// A refund of a sale.
    Refund,
    /// A cash-out.
    CashOut,
    /// Change.
    Change,
}

/// The owner phone's label of an output. It is the owner phone's claim and
/// proves nothing; `is_claim` is always true so that a screen cannot show it
/// as a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerLabelView {
    /// Always true.
    pub is_claim: bool,
    /// What the label says.
    pub kind: OwnerLabelKind,
    /// A refund: the counter that made the sale, in full.
    pub counter: Option<Vec<u8>>,
    /// A refund: the sale's receive index.
    pub sale_index: Option<u32>,
    /// A refund: the cashier's name.
    pub cashier: Option<String>,
    /// A refund: the value in the shop's currency, in minor units.
    pub shop_value_minor: Option<u64>,
    /// A refund: the currency code.
    pub shop_currency: Option<String>,
    /// A cash-out: the destination's label.
    pub destination: Option<String>,
}

/// One output of a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputView {
    /// Position in the transaction.
    pub index: u32,
    /// The address, if the script has one.
    pub address: Option<String>,
    /// The script, as hex.
    pub script_hex: String,
    /// The amount in satoshis.
    pub amount_sats: u64,
    /// What the signer found.
    pub kind: OutputKindView,
    /// The owner phone's label, `None` when it gave none.
    pub owner_phone_label: Option<OwnerLabelView>,
}

/// What would remain under each ceiling, in satoshis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CeilingsRemaining {
    /// Per transaction, other destinations.
    pub other_per_tx: u64,
    /// Per day, other destinations and fees.
    pub other_day: u64,
    /// Per week, other destinations and fees.
    pub other_week: u64,
    /// Per transaction, allowlisted destinations.
    pub allowlisted_per_tx: u64,
    /// Per day, allowlisted destinations.
    pub allowlisted_day: u64,
    /// Per week, allowlisted destinations.
    pub allowlisted_week: u64,
}

/// The signer's review of a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendReviewView {
    /// Every output, change included.
    pub outputs: Vec<OutputView>,
    /// The change index, only if the signer proved it.
    pub change_index: Option<u32>,
    /// The change amount, only if the signer proved it.
    pub change_sats: Option<u64>,
    /// The only output is change.
    pub moves_coins_inside_wallet: bool,
    /// The fee in satoshis.
    pub fee_sats: u64,
    /// The fee rate in satoshis per virtual byte, rounded up.
    pub fee_rate_sat_per_vb: u64,
    /// The number of inputs.
    pub input_count: u32,
    /// The total of the inputs in satoshis.
    pub input_total_sats: u64,
    /// The total to other destinations.
    pub other_total_sats: u64,
    /// The total to allowlisted destinations.
    pub allowlisted_total_sats: u64,
    /// What would remain under each ceiling.
    pub remaining: CeilingsRemaining,
    /// The transaction is in the spend log already and counts once.
    pub already_counted: bool,
    /// How far the clock is behind the latest time seen, if it is.
    pub clock_behind_by: Option<u64>,
}

/// The detail of a transaction line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendDetail {
    /// The review, or `None` when the signer refused the file or key 2 is
    /// not on this device (the line's failure text says which).
    pub review: Option<SpendReviewView>,
    /// Outputs that have no owner-phone label and are not change.
    pub outputs_without_label: Vec<u32>,
    /// Labels that name an output the transaction does not have.
    pub labels_without_output: Vec<u32>,
    /// Outputs the owner phone called change that the signer did not prove
    /// to be change.
    pub claimed_change_not_proven: Vec<u32>,
}

/// The detail of an address batch line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchDetail {
    /// The counter's key, in full.
    pub counter: Vec<u8>,
    /// The counter's four fingerprint words.
    pub counter_words: Vec<String>,
    /// The first index.
    pub start: u32,
    /// One past the last index, if the range is in bounds.
    pub end: Option<u32>,
    /// The number of addresses.
    pub count: u32,
    /// The receive mark the range must start at.
    pub receive_mark: u32,
    /// The checks, in their order.
    pub checks: Vec<CheckView>,
}

/// One settings field that differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingChangeView {
    /// The field's name.
    pub field: String,
    /// The old value, `None` when the Key app co-signed no settings before.
    pub old: Option<String>,
    /// The new value.
    pub new: String,
    /// Loosens or tightens.
    pub direction: Direction,
}

/// The detail of a settings line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsDetail {
    /// The new version.
    pub new_version: u32,
    /// The version of the last settings the Key app co-signed.
    pub last_version: Option<u32>,
    /// Every field that differs.
    pub changes: Vec<SettingChangeView>,
    /// Whether the change as a whole loosens; `None` if the checks failed.
    pub overall: Option<Direction>,
    /// Owner signature, values, version.
    pub checks: Vec<CheckView>,
}

/// The detail of a pairing line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingDetail {
    /// The counter's name.
    pub name: String,
    /// The counter's key, in full.
    pub counter_key: Vec<u8>,
    /// The four session words the counter must show too, when the request
    /// passed its checks.
    pub session_words: Option<Vec<String>>,
    /// The settings version the owner phone claims.
    pub claimed_settings_version: u32,
    /// The settings version the Key app would sign.
    pub settings_version_to_sign: u32,
}

/// The detail of a health check line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthDetail {
    /// The number the Key app would sign.
    pub nonce: Vec<u8>,
}

/// What a line shows, by kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineDetail {
    /// A transaction.
    Spend {
        /// The detail.
        detail: SpendDetail,
    },
    /// An address batch.
    Batch {
        /// The detail.
        detail: BatchDetail,
    },
    /// Settings.
    Settings {
        /// The detail.
        detail: SettingsDetail,
    },
    /// A pairing.
    Pairing {
        /// The detail.
        detail: PairingDetail,
    },
    /// A health check.
    Health {
        /// The detail.
        detail: HealthDetail,
    },
}

/// One line of the session (AS-23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineView {
    /// The item's number, which `decide` takes.
    pub item: u32,
    /// The item's kind.
    pub kind: LineKind,
    /// A short name of the kind, for a heading.
    pub title: String,
    /// Whether the line can be approved.
    pub can_approve: bool,
    /// When it cannot: the failing rule, in the approver's words.
    pub failure: Option<String>,
    /// The decision made so far.
    pub decision: Option<Decision>,
    /// What the checks found.
    pub detail: LineDetail,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn direction(c: Change) -> Direction {
    match c {
        Change::Tightening => Direction::Tightening,
        Change::Loosening => Direction::Loosening,
    }
}

fn checks(steps: &[CheckStep]) -> Vec<CheckView> {
    steps
        .iter()
        .map(|s| CheckView {
            name: s.name.to_string(),
            status: match s.status {
                StepStatus::Passed => CheckStatus::Passed,
                StepStatus::Failed => CheckStatus::Failed,
                StepStatus::NotChecked => CheckStatus::NotChecked,
            },
        })
        .collect()
}

fn label(l: &Label) -> OwnerLabelView {
    let mut v = OwnerLabelView {
        is_claim: true,
        kind: OwnerLabelKind::Change,
        counter: None,
        sale_index: None,
        cashier: None,
        shop_value_minor: None,
        shop_currency: None,
        destination: None,
    };
    match l {
        Label::Refund {
            counter,
            sale_index,
            cashier,
            shop_value,
        } => {
            v.kind = OwnerLabelKind::Refund;
            v.counter = Some(counter.to_bytes().to_vec());
            v.sale_index = Some(*sale_index);
            v.cashier = Some(cashier.clone());
            v.shop_value_minor = shop_value.map(|s| s.minor);
            v.shop_currency = shop_value.map(|s| String::from_utf8_lossy(&s.currency).into_owned());
        }
        Label::CashOut { destination } => {
            v.kind = OwnerLabelKind::CashOut;
            v.destination = Some(destination.clone());
        }
        Label::Change => {}
    }
    v
}

fn output(o: &OutputLine) -> OutputView {
    OutputView {
        index: u32_of(o.index),
        address: o.address.clone(),
        script_hex: o.script_hex.clone(),
        amount_sats: o.amount_sats,
        kind: match &o.kind {
            OutputKind::Change { index } => OutputKindView::Change { index: *index },
            OutputKind::Allowlisted { label } => OutputKindView::Allowlisted {
                label: label.clone(),
            },
            OutputKind::Other => OutputKindView::Other,
        },
        owner_phone_label: o.label_from_owner_phone.as_ref().map(label),
    }
}

fn remaining(r: &Remaining) -> CeilingsRemaining {
    CeilingsRemaining {
        other_per_tx: r.other_per_tx,
        other_day: r.other_day,
        other_week: r.other_week,
        allowlisted_per_tx: r.allowlisted_per_tx,
        allowlisted_day: r.allowlisted_day,
        allowlisted_week: r.allowlisted_week,
    }
}

fn review(r: &SpendReview) -> SpendReviewView {
    SpendReviewView {
        outputs: r.outputs.iter().map(output).collect(),
        change_index: r.change.map(|c| c.0),
        change_sats: r.change.map(|c| c.1),
        moves_coins_inside_wallet: r.moves_coins_inside_wallet,
        fee_sats: r.fee_sats,
        fee_rate_sat_per_vb: r.fee_rate_sat_per_vb,
        input_count: u32_of(r.input_count),
        input_total_sats: r.input_total_sats,
        other_total_sats: r.other_total_sats,
        allowlisted_total_sats: r.allowlisted_total_sats,
        remaining: remaining(&r.remaining),
        already_counted: r.already_counted,
        clock_behind_by: r.clock_behind_by,
    }
}

fn indexes(v: &[usize]) -> Vec<u32> {
    v.iter().map(|i| u32_of(*i)).collect()
}

fn spend(s: &SpendLine) -> SpendDetail {
    SpendDetail {
        review: s.review.as_ref().map(review),
        outputs_without_label: indexes(&s.outputs_without_label),
        labels_without_output: s.labels_without_output.clone(),
        claimed_change_not_proven: indexes(&s.claimed_change_not_proven),
    }
}

fn batch(b: &BatchLine) -> BatchDetail {
    BatchDetail {
        counter: b.counter.to_bytes().to_vec(),
        counter_words: b
            .counter
            .fingerprint_words()
            .iter()
            .map(|w| w.to_string())
            .collect(),
        start: b.start,
        end: b.end,
        count: b.count,
        receive_mark: b.receive_mark,
        checks: checks(&b.checks),
    }
}

fn change(c: &SettingChange) -> SettingChangeView {
    SettingChangeView {
        field: c.field.to_string(),
        old: c.old.clone(),
        new: c.new.clone(),
        direction: direction(c.direction),
    }
}

fn settings(s: &SettingsLine) -> SettingsDetail {
    SettingsDetail {
        new_version: s.new_version,
        last_version: s.last_version,
        changes: s.changes.iter().map(change).collect(),
        overall: s.overall.map(direction),
        checks: checks(&s.checks),
    }
}

fn pairing(p: &PairingLine) -> PairingDetail {
    PairingDetail {
        name: p.name.clone(),
        counter_key: p.counter.to_bytes().to_vec(),
        session_words: p
            .session_words
            .map(|w| w.iter().map(|x| x.to_string()).collect()),
        claimed_settings_version: p.claimed_settings_version,
        settings_version_to_sign: p.settings_version_to_sign,
    }
}

fn health(h: &HealthLine) -> HealthDetail {
    HealthDetail {
        nonce: h.nonce.to_vec(),
    }
}

/// One approver line as the record the app shows.
pub(crate) fn line_view(line: &Line) -> LineView {
    let (kind, title) = match line.kind {
        ItemKind::Spend => (LineKind::Spend, "Transaction"),
        ItemKind::BatchRefill => (LineKind::BatchRefill, "Address batch"),
        ItemKind::Settings => (LineKind::Settings, "Settings"),
        ItemKind::Pairing => (LineKind::Pairing, "Pairing"),
        ItemKind::HealthCheck => (LineKind::HealthCheck, "Health check"),
    };
    let detail = match &line.detail {
        Detail::Spend(s) => LineDetail::Spend { detail: spend(s) },
        Detail::Batch(b) => LineDetail::Batch { detail: batch(b) },
        Detail::Settings(s) => LineDetail::Settings {
            detail: settings(s),
        },
        Detail::Pairing(p) => LineDetail::Pairing { detail: pairing(p) },
        Detail::Health(h) => LineDetail::Health { detail: health(h) },
    };
    LineView {
        item: line.item,
        kind,
        title: title.to_string(),
        can_approve: line.can_approve,
        failure: line.failure.clone(),
        decision: line.decision.as_ref().map(Decision::from),
        detail,
    }
}

/// The script of an output as hex, for screens that list a script.
pub(crate) fn script_hex(bytes: &[u8]) -> String {
    hex(bytes)
}
