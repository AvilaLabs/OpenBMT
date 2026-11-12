//! Adding a Box, from the owner phone's side (AS-83 to AS-87): the claim that
//! is queued with the Box's four words, the settings that add the Box queued
//! with it, and what the owner phone hands the Box once the Key app has
//! approved both.

use openbmt_core::boxpair::{Claim, Welcome, CLAIM_LEN};
use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_core::words::WORD_COUNT;

use crate::edit::sats_per_usd_at;
use crate::error::{BoxGap, OwnerError, Refusal};
use crate::state::{waiting_box, Owner};
use crate::types::{Delivery, QueuedBoxClaim, SettingsQueued, Stage};
use crate::MAX_BOX_CLAIMS;

/// A Box's claim that was queued (AS-83).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxRequest {
    /// The claim's number in the queue.
    pub id: u32,
    /// The Box's key.
    pub box_key: DeviceId,
    /// The four fingerprint words of the Box's key, which the Key app and
    /// the Box show too.
    pub words: [&'static str; WORD_COUNT],
}

/// What was queued for a Box besides its claim (AS-84).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxQueued {
    /// The settings' number in the queue.
    pub settings_id: u32,
    /// The settings' version.
    pub settings_version: u32,
    /// The claim's number in the queue.
    pub claim_id: u32,
}

/// A Box whose finished welcome waits to be handed over (AS-86).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingBox {
    /// The Box's key.
    pub box_key: DeviceId,
}

/// A Box that has a claim queued or a welcome waiting (AS-87).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingBox {
    /// The Box's key.
    pub box_key: DeviceId,
    /// Whether its finished welcome waits to be handed over.
    pub welcome_ready: bool,
}

/// What a Box is handed (AS-86).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxHandover {
    /// The Box's key.
    pub box_key: DeviceId,
    /// The welcome (`OBMM`), 101 bytes.
    pub welcome: Vec<u8>,
    /// The settings with both signatures (`OBMS`) that list the Box.
    pub settings: Vec<u8>,
    /// The counters the settings name, in key order: the devices the shop's
    /// link is made for.
    pub counters: Vec<DeviceId>,
    /// The outbox number of the welcome.
    pub welcome_id: u32,
    /// The outbox number of the settings.
    pub settings_id: u32,
}

impl Owner {
    /// AS-83: queue a Box's claim, read strictly, and give back the four
    /// fingerprint words of the Box's key. The owner phone cannot check the
    /// claim's MAC and does not try (BX-13). The words are made with a
    /// deliberately slow stretch.
    pub fn queue_box_claim<E>(
        &mut self,
        claim_bytes: &[u8],
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<BoxRequest, OwnerError<E>> {
        self.require_key_app()?;
        let claim =
            Claim::from_bytes(claim_bytes).map_err(|e| Refusal::BoxNotClaim(e.to_string()))?;
        if claim.box_key == self.identity.device_id() || claim.box_key == self.roots.approver {
            return Err(Refusal::BoxIsRoot.into());
        }
        if self.state.knows_box(&claim.box_key) {
            return Err(Refusal::BoxKnown.into());
        }
        let mut next = self.state.clone();
        if next.box_claims.len() >= MAX_BOX_CLAIMS {
            return Err(Refusal::Full("the Box claim queue").into());
        }
        let id = next.take_id()?;
        let box_key = claim.box_key;
        next.box_claims.push(QueuedBoxClaim {
            id,
            claim,
            stage: Stage::Queued,
        });
        self.commit(next, persist)?;
        Ok(BoxRequest {
            id,
            box_key,
            words: box_key.fingerprint_words(),
        })
    }

    /// AS-84: the settings that add `box_key`: the latest held or queued
    /// settings (or the starting values at the price `usd_per_btc` (AS-75) when there are
    /// none), with the Box on the device list and among the Boxes, the
    /// counters and their shares as they are, and the version one above
    /// every version signed so far.
    pub fn settings_with_box(
        &self,
        box_key: DeviceId,
        usd_per_btc: u64,
    ) -> Result<Settings, Refusal> {
        let mut settings = match self.state.latest_settings() {
            Some(s) => s.clone(),
            None if usd_per_btc == 0 => return Err(Refusal::NoSettingsRate),
            None => Settings::defaults(sats_per_usd_at(usd_per_btc)?),
        };
        settings.version = self
            .state
            .highest_signed_version()
            .checked_add(1)
            .ok_or_else(|| Refusal::SettingsInvalid("no version is left".to_string()))?;
        settings.devices.insert(box_key);
        settings.boxes.insert(box_key);
        settings
            .validate()
            .map_err(|e| Refusal::SettingsInvalid(e.to_string()))?;
        Ok(settings)
    }

    /// AS-84: for the queued claim `claim`, queue the settings that add its
    /// Box. Both are stored in one write: if the settings are refused, or
    /// the store fails, nothing is queued.
    pub fn queue_box<E>(
        &mut self,
        claim: u32,
        usd_per_btc: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<BoxQueued, OwnerError<E>> {
        let box_key = self
            .state
            .box_claims
            .iter()
            .find(|c| c.id == claim && c.stage == Stage::Queued)
            .map(|c| c.claim.box_key)
            .ok_or(Refusal::UnknownEntry(claim))?;
        let settings = self.settings_with_box(box_key, usd_per_btc)?;
        let version = settings.version;
        let mut next = self.state.clone();
        let settings_id = match self.stage_settings(&mut next, settings)? {
            SettingsQueued::Queued(id) | SettingsQueued::Tightening { id, .. } => id,
        };
        self.commit(next, persist)?;
        Ok(BoxQueued {
            settings_id,
            settings_version: version,
            claim_id: claim,
        })
    }

    /// AS-83: whether the shop knows this Box: it is a device or a Box of the
    /// settings held or queued, has a claim queued, or has a welcome waiting.
    pub fn knows_box(&self, key: &DeviceId) -> bool {
        self.state.knows_box(key)
    }

    /// The finished welcomes that wait, with the Box each is for.
    fn waiting_welcomes(&self) -> Vec<(u32, Claim, Welcome)> {
        self.state
            .outbox
            .iter()
            .filter_map(|d| match &d.what {
                Delivery::BoxWelcome(bytes) => {
                    let claim = Claim::from_bytes(bytes.get(..CLAIM_LEN)?).ok()?;
                    let welcome = Welcome::from_bytes(bytes.get(CLAIM_LEN..)?).ok()?;
                    Some((d.id, claim, welcome))
                }
                _ => None,
            })
            .collect()
    }

    /// AS-86: the Boxes whose finished welcome waits, in the order of the
    /// outbox.
    pub fn boxes_waiting(&self) -> Vec<WaitingBox> {
        self.waiting_welcomes()
            .into_iter()
            .map(|(_, c, _)| WaitingBox { box_key: c.box_key })
            .collect()
    }

    /// AS-87: the Boxes that have a claim queued or a welcome waiting, each
    /// once, in the order of the queue and then the outbox, with whether
    /// the welcome is ready.
    pub fn boxes_pending(&self) -> Vec<PendingBox> {
        let ready: Vec<DeviceId> = self
            .waiting_welcomes()
            .into_iter()
            .map(|(_, c, _)| c.box_key)
            .collect();
        let mut out: Vec<PendingBox> = Vec::new();
        let keys = self
            .state
            .box_claims
            .iter()
            .map(|c| c.claim.box_key)
            .chain(ready.iter().copied());
        for box_key in keys {
            if !out.iter().any(|p| p.box_key == box_key) {
                out.push(PendingBox {
                    box_key,
                    welcome_ready: ready.contains(&box_key),
                });
            }
        }
        out
    }

    /// AS-86: the pieces of `box_key`'s hand-over that wait in the outbox.
    fn handover_parts(&self, box_key: &DeviceId) -> Result<BoxHandover, Refusal> {
        let (welcome_id, _, welcome) = self
            .waiting_welcomes()
            .into_iter()
            .find(|(_, c, _)| c.box_key == *box_key)
            .ok_or(Refusal::BoxNotReady(BoxGap::Welcome))?;
        let mut best: Option<(u32, SignedSettings)> = None;
        for d in &self.state.outbox {
            let Delivery::Settings(bytes) = &d.what else {
                continue;
            };
            let Ok(s) = SignedSettings::from_bytes(bytes) else {
                continue;
            };
            let newer = best
                .as_ref()
                .is_none_or(|(_, b)| s.settings.version > b.settings.version);
            if s.approver_sig.is_some() && s.settings.boxes.contains(box_key) && newer {
                best = Some((d.id, s));
            }
        }
        let (settings_id, settings) = best.ok_or(Refusal::BoxNotReady(BoxGap::Settings))?;
        let counters: Vec<DeviceId> = settings.settings.counters.keys().copied().collect();
        Ok(BoxHandover {
            box_key: *box_key,
            welcome: welcome.to_bytes(),
            settings: settings.to_bytes(),
            counters,
            welcome_id,
            settings_id,
        })
    }

    /// AS-86: what to hand the Box: its welcome and the settings with both
    /// signatures that list it, the highest version of them. Nothing is
    /// changed.
    pub fn box_handover(&self, box_key: &DeviceId) -> Result<BoxHandover, Refusal> {
        self.handover_parts(box_key)
    }

    /// AS-87: forget a Box that was not handed its welcome: its queued claim,
    /// the queued settings that list it, and the welcome and the approved
    /// settings that wait in the outbox for it go in one write. With no claim
    /// queued and no welcome waiting there is nothing to forget, and a Box
    /// that took its welcome has neither. What is in a session cannot be
    /// forgotten until the session is abandoned (AS-54).
    pub fn forget_box<E>(
        &mut self,
        box_key: &DeviceId,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let listed = |s: &SignedSettings| s.settings.boxes.contains(box_key);
        let has_claim = self
            .state
            .box_claims
            .iter()
            .any(|c| c.claim.box_key == *box_key);
        let has_welcome = self.state.outbox.iter().any(
            |d| matches!(&d.what, Delivery::BoxWelcome(b) if waiting_box(b) == Some(*box_key)),
        );
        if !has_claim && !has_welcome {
            return Err(Refusal::BoxNothingToForget.into());
        }
        let in_session = self
            .state
            .box_claims
            .iter()
            .any(|c| c.claim.box_key == *box_key && matches!(c.stage, Stage::InSession { .. }))
            || self
                .state
                .settings
                .iter()
                .any(|s| listed(&s.signed) && matches!(s.stage, Stage::InSession { .. }));
        if in_session {
            return Err(
                Refusal::WrongStage("it is in a session; abandon the session first").into(),
            );
        }
        let mut next = self.state.clone();
        next.box_claims.retain(|c| c.claim.box_key != *box_key);
        next.settings.retain(|s| !listed(&s.signed));
        next.outbox.retain(|d| match &d.what {
            Delivery::BoxWelcome(b) => waiting_box(b) != Some(*box_key),
            Delivery::Settings(b) => !SignedSettings::from_bytes(b).is_ok_and(|s| listed(&s)),
            _ => true,
        });
        self.commit(next, persist)
    }

    /// AS-86: the Box has accepted its welcome (BX-15). The welcome leaves
    /// the outbox, and so do the settings when they list no counter.
    pub fn mark_box_delivered<E>(
        &mut self,
        box_key: &DeviceId,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let parts = self.handover_parts(box_key)?;
        let alone = parts.counters.is_empty();
        let mut next = self.state.clone();
        next.outbox
            .retain(|d| d.id != parts.welcome_id && !(alone && d.id == parts.settings_id));
        self.commit(next, persist)
    }
}
