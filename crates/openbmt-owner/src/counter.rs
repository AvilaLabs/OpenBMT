//! Adding a counter, from the owner phone's side (AS-69 to AS-73): the
//! request that is queued with its four words, the settings that list the
//! counter and the first batch for it, queued together, and the bundle the
//! counter takes when the Key app has approved all three.

use std::collections::{BTreeMap, BTreeSet};

use openbmt_core::batch::{AddressBatch, BatchBuilder, SignedBatch};
use openbmt_core::bundle::CounterBundle;
use openbmt_core::identity::DeviceId;
use openbmt_core::pairing::{check_request, PairOffer, PairRequest, SignedPairing};
use openbmt_core::settings::{Settings, SignedSettings, SHARE_TOTAL_BPS};
use openbmt_core::words::WORD_COUNT;

use crate::error::{BundleGap, OwnerError, Refusal};
use crate::state::{Owner, State};
use crate::types::{Delivery, SettingsQueued, Stage};

/// A counter's request that was queued (AS-69).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterRequest {
    /// The pairing's number in the queue.
    pub id: u32,
    /// The counter's key.
    pub counter: DeviceId,
    /// The name the counter gave itself. It is text from the counter.
    pub name: String,
    /// The four session words, which the counter and the Key app show too.
    pub words: [&'static str; WORD_COUNT],
}

/// What was queued for a counter besides its pairing (AS-70, AS-71).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterQueued {
    /// The settings' number in the queue.
    pub settings_id: u32,
    /// The settings' version.
    pub settings_version: u32,
    /// The first batch's number in the queue.
    pub batch_id: u32,
    /// Each counter's share of the instant limit in the new settings, in
    /// basis points, in the order of the counters' keys.
    pub shares: Vec<(DeviceId, u16)>,
}

/// A counter whose finished pairing waits to be handed over (AS-73).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingCounter {
    /// The counter's key.
    pub counter: DeviceId,
    /// The counter's name.
    pub name: String,
}

/// A counter's bundle, with the finished objects it is made from (AS-72).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterBundleMade {
    /// The bundle.
    pub bundle: CounterBundle,
    /// The outbox numbers it is made from.
    pub parts: BundleParts,
}

/// The outbox numbers a bundle is made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleParts {
    /// The finished pairing.
    pub pairing: u32,
    /// The finished settings.
    pub settings: u32,
    /// The first batch, when one waits.
    pub batch: Option<u32>,
}

/// AS-70: `counters` with the shares split equally: each gets the whole
/// budget divided by their number, rounded down, and the earliest by key
/// gets what is left over. No counters, no shares.
pub fn equal_shares(counters: &BTreeSet<DeviceId>) -> BTreeMap<DeviceId, u16> {
    let n = u32::try_from(counters.len()).unwrap_or(u32::MAX);
    let mut out = BTreeMap::new();
    if n == 0 {
        return out;
    }
    let each = SHARE_TOTAL_BPS / n;
    let left = SHARE_TOTAL_BPS - each * n;
    for (i, c) in counters.iter().enumerate() {
        let share = if i == 0 { each + left } else { each };
        out.insert(*c, u16::try_from(share).unwrap_or(u16::MAX));
    }
    out
}

impl State {
    /// AS-70: the settings with the highest version among the ones held and
    /// the ones queued, whatever their stage.
    pub(crate) fn latest_settings(&self) -> Option<&Settings> {
        self.known
            .iter()
            .map(|k| &k.settings)
            .chain(self.settings.iter().map(|q| &q.signed.settings))
            .max_by_key(|s| s.version)
    }

    /// AS-72: the finished pairings that wait, by outbox number.
    fn waiting_pairings(&self) -> Vec<(u32, SignedPairing)> {
        self.outbox
            .iter()
            .filter_map(|d| match &d.what {
                Delivery::Pairing(bytes) => {
                    SignedPairing::from_bytes(bytes).ok().map(|p| (d.id, p))
                }
                _ => None,
            })
            .collect()
    }
}

impl Owner {
    /// AS-58, AS-69: queue a counter's request, and give back the four
    /// words that the counter and the Key app show for it. The words are
    /// made with a deliberately slow stretch.
    pub fn queue_counter_request<E>(
        &mut self,
        request_bytes: &[u8],
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<CounterRequest, OwnerError<E>> {
        let id = self.queue_pairing(request_bytes, persist)?;
        let request = self
            .state
            .pairings
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.request.clone())
            .ok_or(Refusal::UnknownEntry(id))?;
        let words = self.session_words_of(&request)?;
        Ok(CounterRequest {
            id,
            counter: request.counter,
            name: request.name,
            words,
        })
    }

    /// The four words of a request, which has passed the offer's check.
    fn session_words_of(
        &self,
        request: &PairRequest,
    ) -> Result<[&'static str; WORD_COUNT], Refusal> {
        let offer = PairOffer::new(
            self.identity.device_id(),
            self.roots.approver,
            self.wallet_id,
            self.wallet.network(),
        )
        .map_err(|e| Refusal::PairingRequest(e.to_string()))?;
        let pending =
            check_request(&offer, request).map_err(|e| Refusal::PairingRequest(e.to_string()))?;
        Ok(pending.session_words())
    }

    /// AS-70: whether the owner phone holds or has queued settings to build
    /// on. When it does not, adding a counter needs a rate to start them.
    pub fn has_settings_base(&self) -> bool {
        self.state.latest_settings().is_some()
    }

    /// AS-70: the settings that add `counter`: the latest held or queued
    /// settings (or the starting values at `sats_per_usd` when there are
    /// none), with the counter on the device list and in the counters, the
    /// shares split equally ([`equal_shares`]) and the version one above
    /// every version signed so far.
    pub fn settings_with_counter(
        &self,
        counter: DeviceId,
        sats_per_usd: u64,
    ) -> Result<Settings, Refusal> {
        let mut settings = match self.state.latest_settings() {
            Some(s) => s.clone(),
            None if sats_per_usd == 0 => return Err(Refusal::NoSettingsRate),
            None => Settings::defaults(sats_per_usd),
        };
        settings.version = self
            .state
            .highest_signed_version()
            .checked_add(1)
            .ok_or_else(|| Refusal::SettingsInvalid("no version is left".to_string()))?;
        settings.devices.insert(counter);
        let mut counters: BTreeSet<DeviceId> = settings.counters.keys().copied().collect();
        counters.insert(counter);
        settings.counters = equal_shares(&counters);
        settings
            .validate()
            .map_err(|e| Refusal::SettingsInvalid(e.to_string()))?;
        Ok(settings)
    }

    /// AS-70, AS-71: for the queued pairing `pairing`, queue the settings
    /// that add its counter and the counter's first batch of `count`
    /// addresses, from the first receive index nobody was given. Both are
    /// stored in one write: if either is refused, neither is queued.
    pub fn queue_counter<E>(
        &mut self,
        pairing: u32,
        sats_per_usd: u64,
        count: u32,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<CounterQueued, OwnerError<E>> {
        let counter = self
            .state
            .pairings
            .iter()
            .find(|p| p.id == pairing && p.stage == Stage::Queued)
            .map(|p| p.request.counter)
            .ok_or(Refusal::UnknownEntry(pairing))?;
        let settings = self.settings_with_counter(counter, sats_per_usd)?;
        let version = settings.version;
        let shares: Vec<(DeviceId, u16)> =
            settings.counters.iter().map(|(c, s)| (*c, *s)).collect();
        let batch: AddressBatch = BatchBuilder::new(self.policy)
            .derive(&self.wallet, counter, self.receive_mark(), count)
            .map_err(|_| Refusal::BatchNotDerived)?;
        let mut next = self.state.clone();
        let settings_id = match self.stage_settings(&mut next, settings)? {
            SettingsQueued::Queued(id) | SettingsQueued::Tightening { id, .. } => id,
        };
        let batch_id = self.stage_batch(&mut next, batch, now)?;
        self.commit(next, persist)?;
        Ok(CounterQueued {
            settings_id,
            settings_version: version,
            batch_id,
            shares,
        })
    }

    /// AS-73: the counters whose finished pairing waits to be handed over,
    /// in the order of the outbox.
    pub fn counters_waiting(&self) -> Vec<WaitingCounter> {
        self.state
            .waiting_pairings()
            .into_iter()
            .map(|(_, p)| WaitingCounter {
                counter: p.entry.counter,
                name: p.entry.name,
            })
            .collect()
    }

    /// AS-72: the pieces of `counter`'s bundle that wait in the outbox.
    fn bundle_parts(&self, counter: &DeviceId) -> Result<(BundleParts, CounterBundle), Refusal> {
        let (pairing_id, pairing) = self
            .state
            .waiting_pairings()
            .into_iter()
            .find(|(_, p)| p.entry.counter == *counter)
            .ok_or(Refusal::BundleNotReady(BundleGap::Pairing))?;
        let floor = pairing.entry.settings_version;
        let mut best: Option<(u32, SignedSettings)> = None;
        let mut first: Option<(u32, SignedBatch)> = None;
        for d in &self.state.outbox {
            match &d.what {
                Delivery::Settings(bytes) => {
                    let Ok(s) = SignedSettings::from_bytes(bytes) else {
                        continue;
                    };
                    let lists = s.settings.devices.contains(counter)
                        && s.settings.counters.contains_key(counter);
                    let newer = best
                        .as_ref()
                        .is_none_or(|(_, b)| s.settings.version > b.settings.version);
                    if lists && s.approver_sig.is_some() && s.settings.version > floor && newer {
                        best = Some((d.id, s));
                    }
                }
                Delivery::Batch(bytes) => {
                    let Ok(b) = SignedBatch::from_bytes(bytes) else {
                        continue;
                    };
                    let earlier = first
                        .as_ref()
                        .is_none_or(|(_, f)| b.batch.start < f.batch.start);
                    if b.batch.counter == *counter && earlier {
                        first = Some((d.id, b));
                    }
                }
                Delivery::Transaction { .. } | Delivery::Pairing(_) => {}
            }
        }
        let (settings_id, settings) = best.ok_or(Refusal::BundleNotReady(BundleGap::Settings))?;
        let parts = BundleParts {
            pairing: pairing_id,
            settings: settings_id,
            batch: first.as_ref().map(|(id, _)| *id),
        };
        // No peers for now: the counter finds its own.
        let bundle = CounterBundle::new(pairing, settings, first.map(|(_, b)| b), Vec::new())
            .map_err(|_| Refusal::BundleNotReady(BundleGap::Settings))?;
        Ok((parts, bundle))
    }

    /// AS-72: the bundle for `counter` from what waits in the outbox: its
    /// finished pairing, the settings with both signatures that list it
    /// above the pairing's version (the highest such), and its first batch
    /// (the one that starts earliest) when one waits. Nothing is changed.
    pub fn counter_bundle(&self, counter: &DeviceId) -> Result<CounterBundleMade, Refusal> {
        let (parts, bundle) = self.bundle_parts(counter)?;
        Ok(CounterBundleMade { bundle, parts })
    }

    /// AS-73: the counter has taken its bundle. Its pairing and its first
    /// batch leave the outbox. The settings leave it only when no other
    /// counter is listed in them, because the others have not been handed
    /// those settings yet.
    pub fn mark_counter_delivered<E>(
        &mut self,
        counter: &DeviceId,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let (parts, bundle) = self.bundle_parts(counter)?;
        let alone = bundle.settings.settings.counters.len() == 1;
        let mut next = self.state.clone();
        next.outbox.retain(|d| {
            d.id != parts.pairing && Some(d.id) != parts.batch && !(alone && d.id == parts.settings)
        });
        self.commit(next, persist)
    }
}
