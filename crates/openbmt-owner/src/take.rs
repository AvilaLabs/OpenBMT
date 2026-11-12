//! Taking the Key app's answer (AS-62 to AS-64, AS-67).

use std::convert::Infallible;

use bitcoin::bip32::Xpriv;
use bitcoin::consensus::serialize;
use bitcoin::psbt::Psbt;
use bitcoin::{ScriptBuf, Transaction, Txid};
use openbmt_core::batch::SignedBatch;
use openbmt_core::pairing::{owner_cosign, PairOffer, PairingEntry};
use openbmt_core::settings::SignedSettings;
use openbmt_core::wire::DecodeError;
use openbmt_session::{DeclineCode, Item, Payload, Request, Response, Verdict};
use openbmt_signer::{SignError, SignerState};
use openbmt_spend::{combine, finalize};

use crate::codec;
use crate::error::{OwnerError, Refusal};
use crate::state::{outputs_fit, Frozen, Owner, State};
use crate::types::{Deliverable, Delivery, ItemOutcome, Stage, Taken};
use crate::{MAX_OUTBOX, MAX_SNAPSHOT_BYTES};

/// AS-12: a reason of 1 to 160 bytes with no control character, from any
/// text.
pub(crate) fn clip_reason(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if out.len() + c.len_utf8() > openbmt_session::MAX_REASON_BYTES {
            break;
        }
        out.push(c);
    }
    if out.trim().is_empty() {
        "no reason was given".to_string()
    } else {
        out
    }
}

fn code_text(code: DeclineCode) -> &'static str {
    match code {
        DeclineCode::ByPerson => "declined on the Key app",
        DeclineCode::ChecksFailed => "the Key app's checks failed",
        DeclineCode::NotOnThisDevice => "not on this device",
        DeclineCode::NotDecided => "not decided",
    }
}

/// The numbers of the entries a frozen item holds.
fn entries(f: &Frozen) -> Vec<u32> {
    match f {
        Frozen::Spend {
            refunds, cash_out, ..
        } => refunds.iter().copied().chain(*cash_out).collect(),
        Frozen::Batch(n) | Frozen::Settings(n) | Frozen::Pairing(n) | Frozen::BoxClaim(n) => {
            vec![*n]
        }
        Frozen::Health => Vec::new(),
    }
}

/// AS-67: the outputs are exactly the frozen payments, in order, and the
/// change, if there was one, at the position that was frozen and to the
/// index that was frozen.
pub(crate) fn outputs_are_frozen(
    owner: &Owner,
    tx: &Transaction,
    payments: &[(ScriptBuf, u64)],
    change_index: u32,
    change_position: Option<u32>,
) -> bool {
    if !outputs_fit(&tx.output, payments, change_position) {
        return false;
    }
    let Some(position) = change_position else {
        return true;
    };
    let at = usize::try_from(position)
        .ok()
        .and_then(|p| tx.output.get(p));
    match (at, owner.wallet.change_script(change_index)) {
        (Some(out), Ok(script)) => out.script_pubkey == script,
        _ => false,
    }
}

impl Owner {
    /// AS-62: add key 1 to an approved transaction and finish it.
    fn finish_spend(
        &self,
        key1: &mut SignerState,
        account: &Xpriv,
        now: u64,
        asked: &[u8],
        theirs: &[u8],
        frozen: &Frozen,
    ) -> Result<(Txid, Vec<u8>), String> {
        let Frozen::Spend {
            txid,
            payments,
            change_index,
            change_position,
            ..
        } = frozen
        else {
            return Err("AS-62: the item froze no transaction".to_string());
        };
        let review = key1.review(asked, now).map_err(|e| e.to_string())?;
        let signed = match key1.sign(review, account, now, |_| Ok::<(), Infallible>(())) {
            Ok(s) => s,
            Err(SignError::Refused(r)) => return Err(r.to_string()),
            Err(SignError::Persist(never)) => match never {},
        };
        let base = Psbt::deserialize(&signed.psbt).map_err(|e| e.to_string())?;
        let other = Psbt::deserialize(theirs)
            .map_err(|e| format!("AS-62: the Key app's file is not a PSBT: {e}"))?;
        let both = combine(&base, &[other]).map_err(|e| e.to_string())?;
        let tx = finalize(&self.wallet, &both).map_err(|e| e.to_string())?;
        if tx.compute_txid() != *txid
            || !outputs_are_frozen(self, &tx, payments, *change_index, *change_position)
        {
            return Err(
                "AS-67: the finished transaction is not the one that was frozen".to_string(),
            );
        }
        Ok((*txid, serialize(&tx)))
    }

    /// AS-62 to AS-64: take the Key app's response to an open session.
    ///
    /// The response must be bound to the request kept for the session
    /// (AS-13). For each approved transaction key 1's signer reviews and
    /// signs it, the two signatures are joined, the transaction is finished
    /// and checked, and its outputs must be the frozen ones. Everything
    /// else is completed with the core crate's own functions. A line that
    /// is declined, or that key 1 or a check on this phone refuses, is
    /// returned to the queue with its reason. The new queue and key 1's new
    /// state are handed to `persist` together, and nothing is returned or
    /// changed in memory unless it stores them.
    pub fn take_response<E>(
        &mut self,
        response: &[u8],
        key1: &mut SignerState,
        key1_account: &Xpriv,
        now: u64,
        persist: impl FnOnce(&[u8], &[u8]) -> Result<(), E>,
    ) -> Result<Taken, OwnerError<E>> {
        self.require_key_app()?;
        if response.len() > openbmt_session::MAX_SESSION_BYTES {
            return Err(Refusal::Frame(DecodeError::TooLarge).into());
        }
        let resp = Response::from_bytes(response).map_err(Refusal::Frame)?;
        let at = self
            .state
            .sessions
            .iter()
            .position(|s| s.id == *resp.session_id() && s.own.is_none())
            .ok_or(Refusal::UnknownSession)?;
        let session = self.state.sessions.get(at).ok_or(Refusal::UnknownSession)?;
        let request = Request::from_bytes(&session.request).map_err(Refusal::Frame)?;
        resp.check_against(&request, &self.roots.approver)
            .map_err(Refusal::Bind)?;
        if key1.wallet().canonical_string() != self.wallet.canonical_string() {
            return Err(Refusal::Key1Wallet.into());
        }
        let approved = resp
            .results()
            .iter()
            .filter(|r| matches!(r.verdict, Verdict::Approved(_)))
            .count();
        if self.state.outbox.len() + approved > MAX_OUTBOX {
            return Err(Refusal::OutboxFull.into());
        }

        let highest_signed = self.state.highest_signed_version();
        let mut next: State = self.state.clone();
        let mut k1 = key1.clone();
        let session = next.sessions.remove(at);
        let mut outcomes: Vec<(u32, ItemOutcome)> = Vec::new();
        for result in resp.results() {
            let number = result.number;
            let Some((_, frozen)) = session.frozen.iter().find(|(n, _)| *n == number) else {
                continue;
            };
            let Some(asked) = request.item(number) else {
                continue;
            };
            let ids = entries(frozen);
            // What to do with the entries of this item: finished, or back.
            let outcome: Result<Option<Delivery>, String> = match (&result.verdict, &asked.item) {
                (Verdict::Declined { code, text }, _) => {
                    Err(format!("{}: {text}", code_text(*code)))
                }
                (Verdict::Approved(Payload::Health { .. }), _) => Ok(None),
                (
                    Verdict::Approved(Payload::Spend { psbt: theirs }),
                    Item::Spend { psbt: asked, .. },
                ) => self
                    .finish_spend(&mut k1, key1_account, now, asked, theirs, frozen)
                    .map(|(txid, tx)| {
                        let (refunds, cash_out) = match frozen {
                            Frozen::Spend {
                                refunds, cash_out, ..
                            } => (refunds.clone(), *cash_out),
                            _ => (Vec::new(), None),
                        };
                        Some(Delivery::Transaction {
                            txid,
                            tx,
                            refunds,
                            cash_out,
                        })
                    }),
                (
                    Verdict::Approved(Payload::Batch { sig }),
                    Item::BatchRefill { batch, owner_sig },
                ) => {
                    let signed = SignedBatch {
                        batch: batch.clone(),
                        owner_sig: *owner_sig,
                        approver_sig: *sig,
                    };
                    signed
                        .verify(&self.roots, &self.policy)
                        .map(|()| Some(Delivery::Batch(signed.to_bytes())))
                        .map_err(|e| format!("AS-62: the batch does not verify: {e:?}"))
                }
                (Verdict::Approved(Payload::Settings { sig }), Item::Settings { signed }) => {
                    let both = SignedSettings {
                        settings: signed.settings.clone(),
                        owner_sig: signed.owner_sig,
                        approver_sig: Some(*sig),
                    };
                    let known = next.known.as_ref().map(|k| &k.settings);
                    match both.verify(&self.roots, known) {
                        Ok(_) => {
                            let bytes = both.to_bytes();
                            next.known = Some(both);
                            Ok(Some(Delivery::Settings(bytes)))
                        }
                        Err(e) => Err(format!("AS-62: the settings do not verify: {e}")),
                    }
                }
                (
                    Verdict::Approved(Payload::Pairing {
                        settings_version,
                        sig,
                    }),
                    Item::Pairing {
                        request: pair,
                        settings_version: claimed,
                    },
                ) => {
                    if settings_version < claimed {
                        Err(
                            "AS-62: the Key app signed a lower settings version than claimed"
                                .to_string(),
                        )
                    } else if *settings_version > highest_signed {
                        // A Key app must not push a counter's version floor
                        // out of reach of the settings the owner signed.
                        Err(
                            "AS-62: the Key app signed a higher settings version than the \
                             owner ever signed"
                                .to_string(),
                        )
                    } else {
                        let entry = PairingEntry {
                            wallet: self.wallet_id,
                            counter: pair.counter,
                            name: pair.name.clone(),
                            nonce: pair.nonce,
                            settings_version: *settings_version,
                        };
                        PairOffer::new(
                            self.identity.device_id(),
                            self.roots.approver,
                            self.wallet_id,
                            self.wallet.network(),
                        )
                        .map_err(|e| e.to_string())
                        .and_then(|offer| {
                            owner_cosign(&offer, entry, *sig, &self.identity)
                                .map_err(|e| format!("AS-62: {e}"))
                        })
                        .map(|signed| Some(Delivery::Pairing(signed.to_bytes())))
                    }
                }
                (Verdict::Approved(Payload::BoxWelcome { welcome }), Item::BoxClaim { claim }) => {
                    // AS-85: the welcome names this owner phone and the
                    // pinned Key app, and no other roots.
                    if welcome.owner != self.identity.device_id()
                        || welcome.approver != self.roots.approver
                    {
                        Err("AS-85: the welcome names other roots than this shop's".to_string())
                    } else {
                        let mut both = claim.to_bytes();
                        both.extend_from_slice(&welcome.to_bytes());
                        Ok(Some(Delivery::BoxWelcome(both)))
                    }
                }
                // `check_against` has made sure the kinds fit.
                _ => Err("AS-13: the result does not fit its item".to_string()),
            };
            match outcome {
                Ok(None) => outcomes.push((number, ItemOutcome::Healthy)),
                Ok(Some(what)) => {
                    let id = next.take_id()?;
                    let stage = match &what {
                        Delivery::Transaction { txid, .. } => Some(Stage::Signed { txid: *txid }),
                        _ => None,
                    };
                    match stage {
                        // A transaction's entries stay and wait for it.
                        Some(stage) => {
                            for n in &ids {
                                if let Some(s) = next.stage_mut(*n) {
                                    *s = stage.clone();
                                }
                            }
                        }
                        // A finished batch, settings item or pairing leaves
                        // the queue; its object waits in the outbox.
                        None => {
                            for n in &ids {
                                next.remove_entry(*n);
                            }
                        }
                    }
                    next.outbox.push(Deliverable { id, what });
                    outcomes.push((number, ItemOutcome::Delivered(id)));
                }
                Err(reason) => {
                    let reason = clip_reason(&reason);
                    for n in &ids {
                        if let Some(s) = next.stage_mut(*n) {
                            *s = Stage::Returned {
                                reason: reason.clone(),
                            };
                        }
                    }
                    outcomes.push((number, ItemOutcome::Returned(reason)));
                }
            }
        }
        let owner_bytes = codec::encode(self, &next)?;
        if owner_bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(Refusal::SnapshotTooLarge.into());
        }
        persist(&owner_bytes, &k1.to_bytes()).map_err(OwnerError::Store)?;
        self.state = next;
        *key1 = k1;
        Ok(Taken {
            session_id: session.id,
            outcomes,
        })
    }
}
