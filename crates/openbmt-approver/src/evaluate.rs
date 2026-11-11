//! The Key app's own check of each kind of item (AS-24 to AS-29, AS-33).
//!
//! Each check works on the state `kept` as the earlier approved lines left
//! it, and changes it only when the line is approved and passes (AS-32).

use std::convert::Infallible;

use bitcoin::bip32::Xpriv;

use openbmt_core::batch::{BatchError, CosignError, HighWater};
use openbmt_core::boxpair::{Claim, Welcome};
use openbmt_core::pairing::{check_request, PairOffer};
use openbmt_core::settings::{changed_fields, Change, SettingsError};
use openbmt_session::{health_proof, DeclineCode, Item, Label, OutputLabel, Payload, RequestItem};
use openbmt_signer::{OutputKind, Review, SignError, Signed, SignerState};

use crate::describe::{field_values, hex, shown_name};
use crate::line::{
    BatchLine, BoxClaimLine, CheckStep, Detail, HealthLine, OutputLine, PairingLine, SettingChange,
    SettingsLine, SpendLine, SpendReview, StepStatus,
};
use crate::session::Session;
use crate::state::Kept;

/// Why a line cannot be approved.
pub(crate) struct Block {
    pub code: DeclineCode,
    pub text: String,
}

/// The result of checking one item.
pub(crate) struct Checked {
    pub detail: Detail,
    pub block: Option<Block>,
    /// The signed payload, only when the line was approved and passed.
    pub payload: Option<Payload>,
}

fn blocked(detail: Detail, code: DeclineCode, text: impl Into<String>) -> Checked {
    Checked {
        detail,
        block: Some(Block {
            code,
            text: text.into(),
        }),
        payload: None,
    }
}

pub(crate) fn eval_item(
    session: &Session<'_>,
    index: usize,
    entry: &RequestItem,
    kept: &mut Kept,
    apply: bool,
) -> Checked {
    match &entry.item {
        Item::Spend { psbt, labels } => eval_spend(session, kept, psbt, labels, apply),
        Item::BatchRefill { batch, owner_sig } => {
            eval_batch(session, kept, batch, owner_sig, apply)
        }
        Item::Settings { signed } => eval_settings(session, kept, signed, apply),
        Item::Pairing {
            request,
            settings_version,
        } => eval_pairing(session, index, kept, request, *settings_version, apply),
        Item::BoxClaim { claim } => eval_box_claim(session, index, kept, claim, apply),
        Item::HealthCheck { nonce } => {
            let payload = apply.then(|| Payload::Health {
                sig: health_proof(
                    &session.approver.identity,
                    session.request.session_id(),
                    entry.number,
                    nonce,
                ),
            });
            Checked {
                detail: Detail::Health(HealthLine { nonce: *nonce }),
                block: None,
                payload,
            }
        }
    }
}

// ---- Spend (AS-24, AS-25, AS-36) ------------------------------------------

fn eval_spend(
    session: &Session<'_>,
    kept: &mut Kept,
    psbt: &[u8],
    labels: &[OutputLabel],
    apply: bool,
) -> Checked {
    let empty = SpendLine {
        review: None,
        outputs_without_label: Vec::new(),
        labels_without_output: Vec::new(),
        claimed_change_not_proven: Vec::new(),
    };
    let Some(signer) = kept.signer.as_mut() else {
        return blocked(
            Detail::Spend(empty),
            DeclineCode::NotOnThisDevice,
            "AS-36: not on this device: key 2 is not held here",
        );
    };
    // AS-24: the line comes from the signer's review of the transaction.
    let review = match signer.review(psbt, session.now) {
        Ok(r) => r,
        Err(refusal) => {
            return blocked(
                Detail::Spend(empty),
                DeclineCode::ChecksFailed,
                format!("{}: {refusal}", refusal.rule()),
            )
        }
    };
    let label_of = |output: usize| -> Option<Label> {
        labels
            .iter()
            .find(|l| usize::try_from(l.output).is_ok_and(|o| o == output))
            .map(|l| l.label.clone())
    };
    let outputs: Vec<OutputLine> = review
        .outputs()
        .iter()
        .map(|o| OutputLine {
            index: o.index,
            address: o.address.as_ref().map(|a| a.to_string()),
            script_hex: hex(o.script.as_bytes()),
            amount_sats: o.amount,
            kind: o.kind.clone(),
            label_from_owner_phone: label_of(o.index),
        })
        .collect();
    let outputs_without_label = outputs
        .iter()
        .filter(|o| {
            o.label_from_owner_phone.is_none() && !matches!(o.kind, OutputKind::Change { .. })
        })
        .map(|o| o.index)
        .collect();
    let claimed_change_not_proven = outputs
        .iter()
        .filter(|o| {
            matches!(o.label_from_owner_phone, Some(Label::Change))
                && !matches!(o.kind, OutputKind::Change { .. })
        })
        .map(|o| o.index)
        .collect();
    let labels_without_output: Vec<u32> = labels
        .iter()
        .map(|l| l.output)
        .filter(|o| usize::try_from(*o).map_or(true, |o| o >= outputs.len()))
        .collect();
    let summary = SpendReview {
        outputs,
        change: review.change(),
        moves_coins_inside_wallet: review.moves_coins_inside_wallet(),
        fee_sats: review.fee_sats(),
        fee_rate_sat_per_vb: review.fee_rate_ceil_sat_per_vb(),
        input_count: review.input_count(),
        input_total_sats: review.input_total_sats(),
        other_total_sats: review.other_total_sats(),
        allowlisted_total_sats: review.allowlisted_total_sats(),
        remaining: review.remaining(),
        already_counted: review.already_counted(),
        clock_behind_by: review.clock_behind_by(),
    };
    let detail = Detail::Spend(SpendLine {
        review: Some(summary),
        outputs_without_label,
        labels_without_output: labels_without_output.clone(),
        claimed_change_not_proven,
    });
    // AS-25: labels that do not describe the file.
    if let Some(first) = labels_without_output.first() {
        return blocked(
            detail,
            DeclineCode::ChecksFailed,
            format!("AS-25: a label names output {first}, which the transaction does not have"),
        );
    }
    if !apply {
        return Checked {
            detail,
            block: None,
            payload: None,
        };
    }
    // AS-33: the signer signs under its own rules.
    let Some(key2) = session.approver.key2.as_ref() else {
        return blocked(
            detail,
            DeclineCode::NotOnThisDevice,
            "AS-36: not on this device: key 2 is not held here",
        );
    };
    match sign_spend(signer, review, key2, session.now) {
        Ok(signed) => Checked {
            detail,
            block: None,
            payload: Some(Payload::Spend { psbt: signed.psbt }),
        },
        Err(SignError::Refused(refusal)) => blocked(
            detail,
            DeclineCode::ChecksFailed,
            format!("{}: {refusal}", refusal.rule()),
        ),
        Err(SignError::Persist(never)) => match never {},
    }
}

/// The one place where the Key app asks the signer for a signature. The
/// nonce is RFC 6979, chosen inside `SignerState::sign`; a nonce protocol
/// (anti-exfil) would change only this function.
fn sign_spend(
    signer: &mut SignerState,
    review: Review,
    key2: &Xpriv,
    now: u64,
) -> Result<Signed, SignError<Infallible>> {
    signer.sign(review, key2, now, |_| Ok::<(), Infallible>(()))
}

// ---- BatchRefill (AS-26) --------------------------------------------------

const BATCH_STEPS: [&str; 6] = [
    "the batch names this wallet (full hash)",
    "the batch has a valid size and shape",
    "the owner's signature is valid",
    "the counter is on the device list",
    "the range starts exactly at the receive mark",
    "the addresses the Key app derives are the same",
];

/// Which of the six checks a batch refusal comes from.
fn failing_step(e: &BatchError) -> usize {
    match e {
        BatchError::WrongWallet => 0,
        BatchError::CountTooSmall { .. }
        | BatchError::CountTooLarge
        | BatchError::RangeOutOfBounds
        | BatchError::ScriptCountMismatch
        | BatchError::DuplicateScript
        | BatchError::UnexpectedScriptType => 1,
        BatchError::SameRoot
        | BatchError::OwnerSignatureInvalid
        | BatchError::ApproverSignatureInvalid => 2,
        BatchError::UnknownCounter | BatchError::WrongCounter => 3,
        BatchError::Gap { .. } | BatchError::Overlap { .. } | BatchError::OverlapsExisting => 4,
        BatchError::HashMismatch | BatchError::Wallet(_) => 5,
    }
}

fn steps(names: &[&'static str], failed: Option<usize>) -> Vec<CheckStep> {
    names
        .iter()
        .enumerate()
        .map(|(i, name)| CheckStep {
            name,
            status: match failed {
                None => StepStatus::Passed,
                Some(f) if i < f => StepStatus::Passed,
                Some(f) if i == f => StepStatus::Failed,
                Some(_) => StepStatus::NotChecked,
            },
        })
        .collect()
}

fn eval_batch(
    session: &Session<'_>,
    kept: &mut Kept,
    batch: &openbmt_core::batch::AddressBatch,
    owner_sig: &openbmt_core::identity::Sig,
    apply: bool,
) -> Checked {
    let a = &*session.approver;
    let counters = kept
        .settings
        .as_ref()
        .map(|s| s.settings.counter_set())
        .unwrap_or_default();
    let mut trial_signer = kept.signer.clone();
    let mut trial_mark = kept.high_water;
    let noop = |_: &HighWater| Ok::<(), Infallible>(());
    let result = match trial_signer.as_mut() {
        Some(signer) => signer.cosign_batch(
            &a.batch,
            batch,
            &counters,
            &a.owner,
            owner_sig,
            |_: &[u8]| Ok::<(), Infallible>(()),
        ),
        None => a.batch.check_and_cosign(
            &a.wallet,
            batch,
            &mut trial_mark,
            &counters,
            &a.owner,
            owner_sig,
            noop,
        ),
    };
    let result: Result<_, BatchError> = result.map_err(|e| match e {
        CosignError::Refused(b) => b,
        CosignError::Persist(never) => match never {},
    });
    let failed = result.as_ref().err().map(failing_step);
    let detail = Detail::Batch(BatchLine {
        counter: batch.counter,
        start: batch.start,
        end: batch.end(),
        count: batch.count,
        receive_mark: kept.receive_mark(),
        checks: steps(&BATCH_STEPS, failed),
    });
    match result {
        Err(e) => {
            let step = BATCH_STEPS.get(failing_step(&e)).copied().unwrap_or("");
            blocked(
                detail,
                DeclineCode::ChecksFailed,
                format!("AS-26: {step}: {e}"),
            )
        }
        Ok(sig) => {
            if apply {
                kept.signer = trial_signer;
                kept.high_water = trial_mark;
            }
            Checked {
                detail,
                block: None,
                payload: apply.then_some(Payload::Batch { sig }),
            }
        }
    }
}

// ---- Settings (AS-27) -----------------------------------------------------

const SETTINGS_STEPS: [&str; 3] = [
    "the owner's signature is valid",
    "the values pass the settings rules",
    "the version is above the last co-signed",
];

fn eval_settings(
    session: &Session<'_>,
    kept: &mut Kept,
    signed: &openbmt_core::settings::SignedSettings,
    apply: bool,
) -> Checked {
    let a = &*session.approver;
    let new = &signed.settings;
    let baseline = kept.settings.as_ref().map(|s| s.settings.clone());
    let verdict = match a.roots() {
        Some(roots) => signed.verify(&roots, baseline.as_ref()),
        None => Err(SettingsError::OwnerSignatureInvalid),
    };
    // `ApproverRequired` means every check passed and the change loosens.
    let (failed, overall) = match &verdict {
        Err(SettingsError::OwnerSignatureInvalid) => (Some(0), None),
        Err(SettingsError::Invalid(_)) => (Some(1), None),
        Err(SettingsError::StaleVersion { .. }) => (Some(2), None),
        Err(SettingsError::ApproverSignatureInvalid) => (Some(0), None),
        Err(SettingsError::ApproverRequired) => (None, Some(Change::Loosening)),
        Ok(change) => (None, Some(*change)),
    };
    let new_values = field_values(new);
    let changes: Vec<SettingChange> = match &baseline {
        Some(old) => {
            let old_values = field_values(old);
            // A limit is two numbers (satoshis and dollars), and
            // `changed_fields` names the field once for each that moved. A
            // field is listed once, and loosens if either number does.
            let mut fields: Vec<(&'static str, Change)> = Vec::new();
            for c in changed_fields(old, new) {
                match fields.iter_mut().find(|(f, _)| *f == c.field) {
                    Some((_, d)) => {
                        if c.direction == Change::Loosening {
                            *d = Change::Loosening;
                        }
                    }
                    None => fields.push((c.field, c.direction)),
                }
            }
            fields
                .into_iter()
                .map(|(field, direction)| SettingChange {
                    field: shown_name(field),
                    old: old_values
                        .iter()
                        .find(|(n, _)| *n == field)
                        .map(|(_, v)| v.clone()),
                    new: new_values
                        .iter()
                        .find(|(n, _)| *n == field)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default(),
                    direction,
                })
                .collect()
        }
        // AS-27: with no earlier settings every field is new and loosens.
        None => new_values
            .into_iter()
            .map(|(field, new)| SettingChange {
                field: shown_name(field),
                old: None,
                new,
                direction: Change::Loosening,
            })
            .collect(),
    };
    let detail = Detail::Settings(SettingsLine {
        new_version: new.version,
        last_version: baseline.as_ref().map(|s| s.version),
        changes,
        overall,
        checks: steps(&SETTINGS_STEPS, failed),
    });
    if failed.is_some() {
        let why = match verdict {
            Err(e) => e.to_string(),
            Ok(_) => String::new(),
        };
        return blocked(detail, DeclineCode::ChecksFailed, format!("AS-27: {why}"));
    }
    if !apply {
        return Checked {
            detail,
            block: None,
            payload: None,
        };
    }
    let sig = new.sign(&a.identity);
    let mut co_signed = signed.clone();
    co_signed.approver_sig = Some(sig);
    kept.settings = Some(co_signed);
    Checked {
        detail,
        block: None,
        payload: Some(Payload::Settings { sig }),
    }
}

// ---- Pairing (AS-28) ------------------------------------------------------

fn eval_pairing(
    session: &Session<'_>,
    index: usize,
    kept: &mut Kept,
    request: &openbmt_core::pairing::PairRequest,
    claimed: u32,
    apply: bool,
) -> Checked {
    let a = &*session.approver;
    let own_version = kept.settings.as_ref().map_or(0, |s| s.settings.version);
    let to_sign = claimed.max(own_version);
    let line = |words| PairingLine {
        name: request.name.clone(),
        counter: request.counter,
        session_words: words,
        claimed_settings_version: claimed,
        settings_version_to_sign: to_sign,
    };
    let offer = match PairOffer::new(
        a.owner,
        a.identity.device_id(),
        a.wallet_id,
        a.wallet.network(),
    ) {
        Ok(o) => o,
        Err(e) => {
            return blocked(
                Detail::Pairing(line(None)),
                DeclineCode::ChecksFailed,
                format!("AS-28: {e}"),
            )
        }
    };
    let pending = match check_request(&offer, request) {
        Ok(p) => p,
        Err(e) => {
            return blocked(
                Detail::Pairing(line(None)),
                DeclineCode::ChecksFailed,
                format!("AS-28: {e}"),
            )
        }
    };
    let words = session
        .words
        .get(index)
        .map(|cell| *cell.get_or_init(|| pending.session_words()));
    let detail = Detail::Pairing(line(words));
    if !apply {
        return Checked {
            detail,
            block: None,
            payload: None,
        };
    }
    match pending.approve(&a.identity, to_sign) {
        Ok((_entry, sig)) => Checked {
            detail,
            block: None,
            payload: Some(Payload::Pairing {
                settings_version: to_sign,
                sig,
            }),
        },
        Err(e) => blocked(detail, DeclineCode::ChecksFailed, format!("AS-28: {e}")),
    }
}

// ---- Box claim (AS-81, AS-82) ---------------------------------------------

const CLAIM_STEPS: [&str; 3] = [
    "an invitation of this app is open and has not run out",
    "the claim answers that invitation",
    "the settings that add this Box are approved",
];

fn eval_box_claim(
    session: &Session<'_>,
    index: usize,
    kept: &mut Kept,
    claim: &Claim,
    apply: bool,
) -> Checked {
    let a = &*session.approver;
    // The invitation is spent by the first claim this app looks at with it
    // (BX-11). `kept` is the copy of this evaluation: the finish keeps it,
    // the lines and the decisions throw it away (AS-21).
    let open = kept.invitation.take();
    let live = open.filter(|o| session.now < o.expires_at);
    let words = session
        .words
        .get(index)
        .map(|cell| *cell.get_or_init(|| claim.box_key.fingerprint_words()));
    let line = |failed: Option<usize>| {
        Detail::BoxClaim(BoxClaimLine {
            box_key: claim.box_key,
            words,
            checks: steps(&CLAIM_STEPS, failed),
        })
    };
    let Some(live) = live else {
        return blocked(
            line(Some(0)),
            DeclineCode::ChecksFailed,
            "AS-81: no invitation of this app is open: make a new one",
        );
    };
    if !claim.verify(&live.invitation) {
        return blocked(
            line(Some(1)),
            DeclineCode::ChecksFailed,
            "AS-81: this is not the claim of a Box that holds the invitation: it is spent, make a new one",
        );
    }
    // BX-14: the settings that add the Box are the Key app's own, co-signed
    // by it, here or earlier, and name the Box's key as a Box (BX-20).
    let listed = kept
        .settings
        .as_ref()
        .is_some_and(|s| s.approver_sig.is_some() && s.settings.boxes.contains(&claim.box_key));
    if !listed {
        return blocked(
            line(Some(2)),
            DeclineCode::ChecksFailed,
            "AS-81: the settings that add this Box are not approved: approve that line first",
        );
    }
    let detail = line(None);
    if !apply {
        return Checked {
            detail,
            block: None,
            payload: None,
        };
    }
    let Some(roots) = a.roots() else {
        return blocked(detail, DeclineCode::ChecksFailed, "AS-82: no roots to pin");
    };
    Checked {
        detail,
        block: None,
        payload: Some(Payload::BoxWelcome {
            welcome: Welcome::make(&live.invitation, claim, roots),
        }),
    }
}
