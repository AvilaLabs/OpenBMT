//! A payment through a channel whose funding has no confirmation yet
//! (SL-27, SL-40).
//!
//! A channel the provider opens for a payment may be used before its funding
//! transaction is in a block (`ProviderConfig::zero_conf`). A payment that
//! came through such a channel rests on a transaction not yet in a block:
//! [`unconfirmed_funding`] finds the funding at the claim, and the paid
//! record keeps every one of them, up to [`MAX_FUNDINGS`] (`paid.rs`).
//!
//! The node then watches that funding, and tells the counter, as it tells it
//! of a paid sale, when it got its first confirmation or is lost
//! ([`FundingEvent`]): the channel is gone, because the funding was spent or
//! beaten by another transaction, or no confirmation came within
//! [`LOST_AFTER_BLOCKS`] of the claim. Each event is kept until
//! [`FundingLog::ack`], and a start gives again every one not acknowledged.
//! One blob for each funding transaction, written as the paid events are.

use crate::locks::MutexExt;
use crate::paid::{hex, invalid, unhex, ACKED_KEPT_SECS};
use crate::store::Background;
use lightning::io;
use lightning::util::persist::KVStore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub use crate::paid::MAX_FUNDINGS;

/// Where the watched fundings and their events are kept.
pub const PRIMARY: &str = "funding_events";
/// Blocks after the claim of a payment through a channel after which a
/// funding with no confirmation is lost (SL-27): a day. **[to tune]**
pub const LOST_AFTER_BLOCKS: u32 = 144;
const VERSION: u8 = 1;
const LEN: usize = 1 + 4 + 4 + 1 + 1 + 8;

/// What the node reads of a channel to judge a funding: the state of the
/// library's `ChannelDetails` that matters here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelFunding {
    /// The channel's ID.
    pub channel_id: [u8; 32],
    /// The funding transaction, once the channel has one.
    pub funding_txid: Option<[u8; 32]>,
    /// The confirmations of the funding: 0 while it is in no block.
    pub confirmations: u32,
}

impl ChannelFunding {
    /// What the library says of a channel.
    pub fn of(c: &lightning::ln::channel_state::ChannelDetails) -> Self {
        use bitcoin::hashes::Hash;
        Self {
            channel_id: c.channel_id.0,
            funding_txid: c.funding_txo.map(|o| o.txid.to_byte_array()),
            confirmations: c.confirmations.unwrap_or(0),
        }
    }
}

/// The fundings a payment came through while they had not the confirmations
/// the shop asks (SL-40).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unconfirmed {
    /// Each funding transaction of a channel that carried an HTLC of the
    /// payment and is not confirmed to the shop's depth, once each, in the
    /// order of the HTLCs; at most [`MAX_FUNDINGS`].
    pub txids: Vec<[u8; 32]>,
    /// More such fundings than [`MAX_FUNDINGS`] came: the payment is never
    /// final at once.
    pub more: bool,
    /// An HTLC came through a channel that is no longer listed, and its
    /// funding is not known (SL-27): the payment is one through a lost
    /// funding.
    pub lost_unknown: bool,
}

/// SL-40: every funding transaction of a channel that carried an HTLC of the
/// payment and whose funding has fewer than `depth` confirmations (at least
/// one is needed). A channel that is not in `channels` (it closed between the
/// HTLC and the claim) has lost its funding (SL-27): `last_known` gives the
/// funding the node last saw for such a channel and its confirmations then;
/// that funding is named unless it had the depth, and with none known the
/// payment is flagged [`Unconfirmed::lost_unknown`].
pub fn unconfirmed_fundings(
    htlc_channels: &[[u8; 32]],
    channels: &[ChannelFunding],
    depth: u32,
    last_known: impl Fn(&[u8; 32]) -> Option<([u8; 32], u32)>,
) -> Unconfirmed {
    let depth = depth.max(1);
    let mut out = Unconfirmed::default();
    for id in htlc_channels {
        let txid = if channels.iter().any(|c| &c.channel_id == id) {
            let Some(txid) = channels
                .iter()
                .find(|c| &c.channel_id == id && c.confirmations < depth)
                .and_then(|c| c.funding_txid)
            else {
                continue;
            };
            txid
        } else {
            match last_known(id) {
                Some((_, confirmations)) if confirmations >= depth => continue,
                Some((txid, _)) => txid,
                None => {
                    out.lost_unknown = true;
                    continue;
                }
            }
        };
        if out.txids.contains(&txid) {
            continue;
        }
        if out.txids.len() == MAX_FUNDINGS {
            out.more = true;
        } else {
            out.txids.push(txid);
        }
    }
    out
}

/// The height of the block a funding is seen in now: from its listed
/// channels when it has any, else the `kept` one.
fn seen_height(
    txid: &[u8; 32],
    kept: Option<u32>,
    channels: &[ChannelFunding],
    height: u32,
) -> Option<u32> {
    let mut listed = channels
        .iter()
        .filter(|c| c.funding_txid.as_ref() == Some(txid))
        .peekable();
    if listed.peek().is_none() {
        return kept;
    }
    listed
        .filter_map(|c| block_height(height, c.confirmations))
        .min()
}

/// The height of the block a funding named by [`unconfirmed_fundings`] was
/// last seen in, through the channels of the payment (`htlc_channels`): from
/// the confirmations `last_known` holds and the tip height they were read at.
/// The caller notes the channels listed at the claim first, so a channel
/// still open gives the block of now, and one gone the block it was last seen
/// in. `None` when it was last seen with none, or not known (SL-27).
pub fn last_seen_height(
    txid: &[u8; 32],
    htlc_channels: &[[u8; 32]],
    last_known: impl Fn(&[u8; 32]) -> Option<([u8; 32], u32, u32)>,
) -> Option<u32> {
    htlc_channels
        .iter()
        .filter_map(last_known)
        .filter(|(t, _, _)| t == txid)
        .filter_map(|(_, confirmations, read)| block_height(read, confirmations))
        .min()
}

/// The channels listed whose funding the log holds as lost, either way: each
/// is force closed at every pass until it is gone, so that a close that
/// failed, or was not reached before a stop, is tried again (SL-27).
pub fn lost_channels(
    channels: &[ChannelFunding],
    is_lost: impl Fn(&[u8; 32]) -> bool,
) -> Vec<[u8; 32]> {
    channels
        .iter()
        .filter(|c| c.funding_txid.as_ref().is_some_and(&is_lost))
        .map(|c| c.channel_id)
        .collect()
}

/// Why a funding is lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LostWhy {
    /// The channel closed before its funding was in a block: the funding was
    /// spent or beaten by another transaction (SL-27).
    Closed,
    /// No confirmation within [`LOST_AFTER_BLOCKS`] of the claim.
    TimedOut,
}

/// What became of a watched funding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not yet in a block, and not yet lost.
    Watching,
    /// The first confirmation.
    Confirmed,
    /// Lost.
    Lost(LostWhy),
}

impl Outcome {
    fn to_byte(self) -> u8 {
        match self {
            Self::Watching => 0,
            Self::Confirmed => 1,
            Self::Lost(LostWhy::Closed) => 2,
            Self::Lost(LostWhy::TimedOut) => 3,
        }
    }

    fn from_byte(b: u8) -> Option<Self> {
        [
            Self::Watching,
            Self::Confirmed,
            Self::Lost(LostWhy::Closed),
            Self::Lost(LostWhy::TimedOut),
        ]
        .into_iter()
        .find(|o| o.to_byte() == b)
    }
}

/// The height of the block a funding is in, from a channel's confirmations
/// seen with the tip at `height`: `None` while it has none.
pub fn block_height(height: u32, confirmations: u32) -> Option<u32> {
    (confirmations >= 1).then(|| height.saturating_sub(confirmations - 1).max(1))
}

/// The state of a funding claimed at `claimed_height`, with the node at
/// `height` and `channels` as the library lists them (SL-27): confirmed when
/// a channel of that funding has `depth` confirmations, the shop's depth (at
/// least one); lost when no channel has it any more and it was never seen in
/// a block (`seen`, the height of that block), or when [`LOST_AFTER_BLOCKS`]
/// have passed since the claim and it has none. One in a block with fewer
/// than `depth` is watched on, and one whose confirmations fall back to none
/// is watched as before. With its channel gone, one that was seen in a block
/// is confirmed once the tip is `depth` blocks deep over it: the channel
/// closed after the funding, not instead of it.
pub fn judge(
    txid: &[u8; 32],
    claimed_height: u32,
    seen: Option<u32>,
    height: u32,
    channels: &[ChannelFunding],
    depth: u32,
) -> Outcome {
    let depth = depth.max(1);
    let mut any = false;
    let mut in_a_block = false;
    for c in channels
        .iter()
        .filter(|c| c.funding_txid.as_ref() == Some(txid))
    {
        any = true;
        if c.confirmations >= depth {
            return Outcome::Confirmed;
        }
        in_a_block |= c.confirmations >= 1;
    }
    if !any {
        return match seen {
            None => Outcome::Lost(LostWhy::Closed),
            Some(s) if height >= s.saturating_add(depth - 1) => Outcome::Confirmed,
            Some(_) => Outcome::Watching,
        };
    }
    if !in_a_block && height >= claimed_height.saturating_add(LOST_AFTER_BLOCKS) {
        return Outcome::Lost(LostWhy::TimedOut);
    }
    Outcome::Watching
}

/// What the log holds of a funding, as the counter asks (SL-27, SL-40).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FundingState {
    /// Watched, with no outcome yet.
    Watching,
    /// Confirmed to the shop's depth.
    Confirmed,
    /// Lost.
    Lost,
    /// Not kept: never watched, or its mark was dropped long after its
    /// event was acknowledged.
    Unknown,
}

/// What the counter is told of a funding it was given in a paid event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FundingEvent {
    /// The funding transaction, as the paid events name it.
    pub txid: [u8; 32],
    /// What became of it: never [`Outcome::Watching`].
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Kept {
    claimed_height: u32,
    /// The height of the block the funding was first seen in, if it was.
    seen_height: Option<u32>,
    outcome: Outcome,
    acked_unix: Option<u64>,
}

fn encode(k: &Kept) -> Vec<u8> {
    let mut out = Vec::with_capacity(LEN);
    out.push(VERSION);
    out.extend_from_slice(&k.claimed_height.to_be_bytes());
    // A height of 0 is not a seen one: 0 is none.
    out.extend_from_slice(&k.seen_height.unwrap_or(0).to_be_bytes());
    out.push(k.outcome.to_byte());
    out.push(k.acked_unix.is_some() as u8);
    out.extend_from_slice(&k.acked_unix.unwrap_or(0).to_be_bytes());
    out
}

fn decode(bytes: &[u8]) -> Result<Kept, io::Error> {
    // The length is checked first, so every field is there.
    let fields = (|| {
        if bytes.len() != LEN || *bytes.first()? != VERSION {
            return None;
        }
        let height = u32::from_be_bytes(bytes.get(1..5)?.try_into().ok()?);
        let seen = u32::from_be_bytes(bytes.get(5..9)?.try_into().ok()?);
        let outcome = *bytes.get(9)?;
        let flag = *bytes.get(10)?;
        let acked = u64::from_be_bytes(bytes.get(11..19)?.try_into().ok()?);
        Some((height, seen, outcome, flag, acked))
    })();
    let Some((claimed_height, seen, outcome, flag, acked)) = fields else {
        return Err(invalid("funding event"));
    };
    let outcome = Outcome::from_byte(outcome).ok_or_else(|| invalid("funding event: outcome"))?;
    // A flag is 0 or 1, and with 0 its eight bytes are zero.
    if flag > 1 {
        return Err(invalid("funding event: flag"));
    }
    if flag == 0 && acked != 0 {
        return Err(invalid("funding event: unused bytes"));
    }
    // What was never settled has nobody to acknowledge it.
    if outcome == Outcome::Watching && flag == 1 {
        return Err(invalid("funding event: acknowledged while watched"));
    }
    Ok(Kept {
        claimed_height,
        seen_height: (seen != 0).then_some(seen),
        outcome,
        acked_unix: (flag == 1).then_some(acked),
    })
}

/// The funding blob read from `bytes` and written again, for the fuzz
/// target: the result must be the input.
#[cfg(feature = "testing")]
pub fn reencode_for_test(bytes: &[u8]) -> Option<Vec<u8>> {
    decode(bytes).ok().map(|k| encode(&k))
}

/// The fundings the node watches for the counter, and their events.
pub struct FundingLog {
    store: Arc<Background>,
    /// What each funding read or written last holds, for [`FundingLog::state`].
    states: Mutex<HashMap<[u8; 32], Outcome>>,
    /// One watch or check at a time: a check reads and then writes, and two
    /// must not overwrite each other's outcome.
    gate: tokio::sync::Mutex<()>,
}

impl FundingLog {
    /// A log over the node's background store.
    pub fn new(store: Arc<Background>) -> Self {
        FundingLog {
            store,
            states: Mutex::new(HashMap::new()),
            gate: tokio::sync::Mutex::new(()),
        }
    }

    fn remember(&self, txid: &[u8; 32], outcome: Option<Outcome>) {
        let mut states = self.states.lock_recover();
        match outcome {
            Some(o) => states.insert(*txid, o),
            None => states.remove(txid),
        };
    }

    /// Reads every funding kept, so that [`FundingLog::state`] knows them.
    /// Called once at a start.
    pub async fn load(&self) -> Result<(), io::Error> {
        for name in KVStore::list(&*self.store, PRIMARY, "").await? {
            if let Some(txid) = unhex(&name) {
                self.read(&txid).await?;
            }
        }
        Ok(())
    }

    /// What the log holds of a funding, now (SL-27, SL-40). Settled
    /// fundings stay known after the counter has acknowledged their events.
    pub fn state(&self, txid: &[u8; 32]) -> FundingState {
        let states = self.states.lock_recover();
        match states.get(txid) {
            None => FundingState::Unknown,
            Some(Outcome::Watching) => FundingState::Watching,
            Some(Outcome::Confirmed) => FundingState::Confirmed,
            Some(Outcome::Lost(_)) => FundingState::Lost,
        }
    }

    async fn read(&self, txid: &[u8; 32]) -> Result<Option<Kept>, io::Error> {
        match KVStore::read(&*self.store, PRIMARY, "", &hex(txid)).await {
            Ok(bytes) => {
                let k = decode(&bytes)?;
                self.remember(txid, Some(k.outcome));
                Ok(Some(k))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn write(&self, txid: &[u8; 32], k: &Kept) -> Result<(), io::Error> {
        KVStore::write(&*self.store, PRIMARY, "", &hex(txid), encode(k)).await?;
        self.remember(txid, Some(k.outcome));
        Ok(())
    }

    /// Starts watching a funding claimed at `height`. One that is kept
    /// already, in any state, is left as it is; if it is settled, its outcome
    /// is returned, to be given to the counter again: the counter may have
    /// acknowledged the event before the payment that names it came.
    pub async fn watch(&self, txid: [u8; 32], height: u32) -> Result<Option<Outcome>, io::Error> {
        self.watch_seen(txid, height, None).await
    }

    /// [`FundingLog::watch`] for a funding already known to have been in a
    /// block: `seen` is the height of that block, kept as its seen height
    /// when the funding is not kept yet, or is watched with none (SL-27).
    pub async fn watch_seen(
        &self,
        txid: [u8; 32],
        height: u32,
        seen: Option<u32>,
    ) -> Result<Option<Outcome>, io::Error> {
        let _one = self.gate.lock().await;
        if let Some(kept) = self.read(&txid).await? {
            if kept.outcome == Outcome::Watching && kept.seen_height.is_none() && seen.is_some() {
                self.write(
                    &txid,
                    &Kept {
                        seen_height: seen,
                        ..kept
                    },
                )
                .await?;
            }
            return Ok((kept.outcome != Outcome::Watching).then_some(kept.outcome));
        }
        self.write(
            &txid,
            &Kept {
                claimed_height: height,
                seen_height: seen,
                outcome: Outcome::Watching,
                acked_unix: None,
            },
        )
        .await?;
        Ok(None)
    }

    /// The fundings still watched, with the height of their claim.
    pub async fn watching(&self) -> Result<Vec<([u8; 32], u32)>, io::Error> {
        let kept = self.watching_kept().await?;
        Ok(kept
            .into_iter()
            .map(|(t, k)| (t, k.claimed_height))
            .collect())
    }

    async fn watching_kept(&self) -> Result<Vec<([u8; 32], Kept)>, io::Error> {
        let mut out = Vec::new();
        for name in KVStore::list(&*self.store, PRIMARY, "").await? {
            let Some(txid) = unhex(&name) else { continue };
            if let Some(k) = self.read(&txid).await? {
                if k.outcome == Outcome::Watching {
                    out.push((txid, k));
                }
            }
        }
        out.sort_by_key(|(t, _)| *t);
        Ok(out)
    }

    /// A watched funding got its outcome: kept before the caller is told.
    /// `true` when it was watched and is now settled; a funding that is not
    /// kept, or is settled already, is not an event again.
    pub async fn settle(&self, txid: &[u8; 32], outcome: Outcome) -> Result<bool, io::Error> {
        let Some(kept) = self.read(txid).await? else {
            return Ok(false);
        };
        if kept.outcome != Outcome::Watching || outcome == Outcome::Watching {
            return Ok(false);
        }
        self.write(txid, &Kept { outcome, ..kept }).await?;
        Ok(true)
    }

    /// Judges every funding watched against `channels` at `height` (SL-27).
    /// Each that got its first confirmation or is lost is kept as an event
    /// first and then returned, once: the caller gives it to the counter.
    pub async fn check(
        &self,
        channels: &[ChannelFunding],
        height: u32,
        depth: u32,
    ) -> Result<Vec<FundingEvent>, io::Error> {
        // One check at a time (SL-27): two that read the same funding as
        // watched must not each write an outcome.
        let _one = self.gate.lock().await;
        let mut out = Vec::new();
        for (txid, kept) in self.watching_kept().await? {
            // The block the funding is seen in is kept, so that a channel
            // that closes after it is not a funding that was lost. While a
            // channel of the funding is listed it is worked out again at
            // every pass, since a reorg can move the block or remove it; the
            // kept height is used only when no channel of it is listed.
            let seen = seen_height(&txid, kept.seen_height, channels, height);
            if seen != kept.seen_height {
                self.write(
                    &txid,
                    &Kept {
                        seen_height: seen,
                        ..kept
                    },
                )
                .await?;
            }
            let outcome = judge(&txid, kept.claimed_height, seen, height, channels, depth);
            if self.settle(&txid, outcome).await? {
                out.push(FundingEvent { txid, outcome });
            }
        }
        Ok(out)
    }

    /// The events not acknowledged, in the order of their funding. The marks
    /// of events acknowledged more than [`ACKED_KEPT_SECS`] before `now` are
    /// dropped.
    pub async fn pending(&self, now: u64) -> Result<Vec<FundingEvent>, io::Error> {
        let mut out = Vec::new();
        for name in KVStore::list(&*self.store, PRIMARY, "").await? {
            let Some(txid) = unhex(&name) else { continue };
            let Some(kept) = self.read(&txid).await? else {
                continue;
            };
            match (kept.outcome, kept.acked_unix) {
                (Outcome::Watching, _) => {}
                (outcome, None) => out.push(FundingEvent { txid, outcome }),
                (_, Some(at)) if at.saturating_add(ACKED_KEPT_SECS) <= now => {
                    KVStore::remove(&*self.store, PRIMARY, "", &name, false).await?;
                    self.remember(&txid, None);
                }
                (_, Some(_)) => {}
            }
        }
        out.sort_by_key(|e| e.txid);
        Ok(out)
    }

    /// The caller has the event: it is not given again. One that is not kept
    /// or not settled is not an error.
    pub async fn ack(&self, txid: &[u8; 32], now: u64) -> Result<(), io::Error> {
        let Some(kept) = self.read(txid).await? else {
            return Ok(());
        };
        if kept.outcome == Outcome::Watching || kept.acked_unix.is_some() {
            return Ok(());
        }
        self.write(
            txid,
            &Kept {
                acked_unix: Some(now),
                ..kept
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{disks, start_background, Disks};

    fn chan(id: u8, txid: Option<u8>, confirmations: u32) -> ChannelFunding {
        ChannelFunding {
            channel_id: [id; 32],
            funding_txid: txid.map(|t| [t; 32]),
            confirmations,
        }
    }

    fn start(d: &Disks) -> (FundingLog, tokio::task::JoinHandle<()>) {
        let (background, flusher) = start_background(d);
        (FundingLog::new(background), flusher)
    }

    const NOW: u64 = 1_000;

    fn ids(n: &[u8]) -> Vec<[u8; 32]> {
        n.iter().map(|t| [*t; 32]).collect()
    }

    #[test]
    fn sl_40_a_payment_through_channels_with_no_confirmation_names_every_funding() {
        let channels = [
            chan(1, Some(11), 3),
            chan(2, Some(22), 0),
            chan(4, Some(44), 0),
        ];
        // Through the confirmed channel: none.
        let none = unconfirmed_fundings(&[[1; 32]], &channels, 1, |_| None);
        assert_eq!((none.txids.len(), none.more), (0, false));
        // Through the other, or through both: the unconfirmed one.
        let one = unconfirmed_fundings(&[[2; 32]], &channels, 1, |_| None);
        assert_eq!(one.txids, ids(&[22]));
        let two = unconfirmed_fundings(&[[1; 32], [2; 32], [2; 32], [4; 32]], &channels, 1, |_| {
            None
        });
        assert_eq!((two.txids, two.more), (ids(&[22, 44]), false));
        // A channel with no funding yet is not one.
        assert!(
            unconfirmed_fundings(&[[3; 32]], &[chan(3, None, 0)], 1, |_| None)
                .txids
                .is_empty()
        );
        assert!(unconfirmed_fundings(&[], &channels, 1, |_| None)
            .txids
            .is_empty());
        // The shop's depth: 3 confirmations are not 4.
        assert!(unconfirmed_fundings(&[[1; 32]], &channels, 3, |_| None)
            .txids
            .is_empty());
        assert_eq!(
            unconfirmed_fundings(&[[1; 32]], &channels, 4, |_| None).txids,
            ids(&[11])
        );
    }

    #[test]
    fn sl_40_more_than_eight_unconfirmed_fundings_are_listed_eight_and_say_more() {
        let channels: Vec<ChannelFunding> = (1..=10).map(|i| chan(i, Some(100 + i), 0)).collect();
        let all: Vec<[u8; 32]> = (1..=10).map(|i| [i; 32]).collect();
        let got = unconfirmed_fundings(&all, &channels, 1, |_| None);
        assert_eq!(got.txids.len(), MAX_FUNDINGS);
        assert_eq!(got.txids[7], [108; 32]);
        assert!(got.more);
        // Exactly eight is not more.
        let got = unconfirmed_fundings(&all[..8], &channels, 1, |_| None);
        assert_eq!((got.txids.len(), got.more), (8, false));
    }

    #[test]
    fn sl_27_a_funding_is_confirmed_at_its_first_confirmation() {
        let t = [22; 32];
        let waiting = [chan(2, Some(22), 0)];
        assert_eq!(judge(&t, 100, None, 100, &waiting, 1), Outcome::Watching);
        assert_eq!(judge(&t, 100, None, 130, &waiting, 1), Outcome::Watching);
        assert_eq!(
            judge(&t, 100, None, 130, &[chan(2, Some(22), 1)], 1),
            Outcome::Confirmed
        );
        // A confirmation after the deadline is still a confirmation.
        assert_eq!(
            judge(&t, 100, None, 999, &[chan(2, Some(22), 1)], 1),
            Outcome::Confirmed
        );
    }

    #[test]
    fn sl_27_a_funding_is_lost_when_its_channel_is_gone_or_144_blocks_pass() {
        let t = [22; 32];
        let waiting = [chan(2, Some(22), 0), chan(1, Some(11), 5)];
        // The channel closed: the funding was spent or beaten.
        assert_eq!(
            judge(&t, 100, None, 101, &[chan(1, Some(11), 5)], 1),
            Outcome::Lost(LostWhy::Closed)
        );
        assert_eq!(
            judge(&t, 100, None, 101, &[], 1),
            Outcome::Lost(LostWhy::Closed)
        );
        // 143 blocks after the claim is still waiting; 144 is lost.
        assert_eq!(judge(&t, 100, None, 243, &waiting, 1), Outcome::Watching);
        assert_eq!(
            judge(&t, 100, None, 100 + LOST_AFTER_BLOCKS, &waiting, 1),
            Outcome::Lost(LostWhy::TimedOut)
        );
    }

    #[tokio::test]
    async fn sl_27_the_check_gives_the_first_confirmation_and_the_loss_once_each() {
        let d = disks("funding-check");
        let (log, _f) = start(&d);
        for t in [1u8, 2, 3, 4] {
            log.watch([t; 32], 100).await.unwrap();
        }
        let channels = [
            chan(1, Some(1), 0),
            chan(2, Some(2), 2),
            chan(3, Some(3), 0),
        ];
        // At 120: 1 and 3 wait, 2 is confirmed, 4's channel is gone.
        let got = log.check(&channels, 120, 1).await.unwrap();
        assert_eq!(
            got,
            vec![
                FundingEvent {
                    txid: [2; 32],
                    outcome: Outcome::Confirmed
                },
                FundingEvent {
                    txid: [4; 32],
                    outcome: Outcome::Lost(LostWhy::Closed)
                }
            ]
        );
        // The same again gives nothing new.
        assert!(log.check(&channels, 121, 1).await.unwrap().is_empty());
        // 3 confirms; 1 reaches the deadline.
        let channels = [chan(1, Some(1), 0), chan(3, Some(3), 1)];
        let got = log
            .check(&channels, 100 + LOST_AFTER_BLOCKS, 1)
            .await
            .unwrap();
        assert_eq!(
            got,
            vec![
                FundingEvent {
                    txid: [1; 32],
                    outcome: Outcome::Lost(LostWhy::TimedOut)
                },
                FundingEvent {
                    txid: [3; 32],
                    outcome: Outcome::Confirmed
                }
            ]
        );
        assert!(log.watching().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn sl_27_an_outcome_is_an_event_once_and_comes_again_after_a_restart_until_acked() {
        let d = disks("funding-events");
        let (log, flusher) = start(&d);
        log.watch([1; 32], 100).await.unwrap();
        log.watch([2; 32], 105).await.unwrap();
        // A second watch changes nothing.
        log.watch([1; 32], 999).await.unwrap();
        assert_eq!(
            log.watching().await.unwrap(),
            vec![([1; 32], 100), ([2; 32], 105)]
        );
        assert!(log.pending(NOW).await.unwrap().is_empty());
        assert!(log.settle(&[1; 32], Outcome::Confirmed).await.unwrap());
        assert!(log
            .settle(&[2; 32], Outcome::Lost(LostWhy::TimedOut))
            .await
            .unwrap());
        // Settled: not an event twice, not watched, not a funding not kept.
        assert!(!log.settle(&[1; 32], Outcome::Confirmed).await.unwrap());
        assert!(!log.settle(&[9; 32], Outcome::Confirmed).await.unwrap());
        assert!(log.watching().await.unwrap().is_empty());
        flusher.abort();
        drop(log);
        let (log, flusher) = start(&d);
        let again = log.pending(NOW).await.unwrap();
        assert_eq!(
            again,
            vec![
                FundingEvent {
                    txid: [1; 32],
                    outcome: Outcome::Confirmed
                },
                FundingEvent {
                    txid: [2; 32],
                    outcome: Outcome::Lost(LostWhy::TimedOut)
                }
            ]
        );
        log.ack(&[1; 32], NOW).await.unwrap();
        // An ack twice, of what is not kept, or of what is watched is not an error.
        log.ack(&[1; 32], NOW).await.unwrap();
        log.ack(&[9; 32], NOW).await.unwrap();
        flusher.abort();
        drop(log);
        let (log, _f) = start(&d);
        assert_eq!(log.pending(NOW).await.unwrap().len(), 1);
        // The watch of an acknowledged funding does not bring its event back.
        log.watch([1; 32], 500).await.unwrap();
        assert!(log.watching().await.unwrap().is_empty());
        // The mark is dropped long after.
        assert_eq!(log.pending(NOW + ACKED_KEPT_SECS).await.unwrap().len(), 1);
        assert!(!d
            .dir
            .join("local")
            .join(PRIMARY)
            .join(hex(&[1; 32]))
            .exists());
    }

    #[test]
    fn sl_40_a_funding_is_confirmed_only_at_the_shops_depth_and_falls_back_to_watching() {
        let t = [22; 32];
        // 1 and 2 confirmations are not 3: still watched, and not lost by
        // the deadline, for it is in a block.
        assert_eq!(
            judge(&t, 100, None, 101, &[chan(2, Some(22), 1)], 3),
            Outcome::Watching
        );
        assert_eq!(
            judge(
                &t,
                100,
                None,
                100 + LOST_AFTER_BLOCKS,
                &[chan(2, Some(22), 2)],
                3
            ),
            Outcome::Watching
        );
        assert_eq!(
            judge(&t, 100, None, 101, &[chan(2, Some(22), 3)], 3),
            Outcome::Confirmed
        );
        // After a reorg it is back to none: watched as before, lost at the deadline.
        assert_eq!(
            judge(&t, 100, None, 130, &[chan(2, Some(22), 0)], 3),
            Outcome::Watching
        );
        assert_eq!(
            judge(
                &t,
                100,
                None,
                100 + LOST_AFTER_BLOCKS,
                &[chan(2, Some(22), 0)],
                3
            ),
            Outcome::Lost(LostWhy::TimedOut)
        );
        // A depth of 0 is 1.
        assert_eq!(
            judge(&t, 100, None, 101, &[chan(2, Some(22), 1)], 0),
            Outcome::Confirmed
        );
    }

    #[tokio::test]
    async fn sl_40_the_check_waits_for_the_depth_and_the_log_says_each_state() {
        let d = disks("funding-depth");
        let (log, _f) = start(&d);
        assert_eq!(log.state(&[1; 32]), FundingState::Unknown);
        log.watch([1; 32], 100).await.unwrap();
        log.watch([2; 32], 100).await.unwrap();
        log.watch([3; 32], 100).await.unwrap();
        assert_eq!(log.state(&[1; 32]), FundingState::Watching);
        let channels = [
            chan(1, Some(1), 1),
            chan(2, Some(2), 2),
            chan(3, Some(3), 0),
        ];
        let got = log.check(&channels, 110, 2).await.unwrap();
        assert_eq!(
            got,
            vec![FundingEvent {
                txid: [2; 32],
                outcome: Outcome::Confirmed
            }]
        );
        assert_eq!(log.state(&[1; 32]), FundingState::Watching);
        assert_eq!(log.state(&[2; 32]), FundingState::Confirmed);
        let got = log.check(&[chan(1, Some(1), 2)], 111, 2).await.unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(log.state(&[1; 32]), FundingState::Confirmed);
        assert_eq!(log.state(&[3; 32]), FundingState::Lost);
        // A state is known after its event is acknowledged, and after a start
        // that loads the log.
        log.ack(&[3; 32], NOW).await.unwrap();
        assert_eq!(log.state(&[3; 32]), FundingState::Lost);
    }

    #[tokio::test]
    async fn sl_27_a_watch_of_a_settled_funding_gives_its_outcome_again_and_a_start_knows_it() {
        let d = disks("funding-rewatch");
        let (log, flusher) = start(&d);
        assert_eq!(log.watch([1; 32], 100).await.unwrap(), None);
        assert_eq!(log.watch([1; 32], 120).await.unwrap(), None);
        log.settle(&[1; 32], Outcome::Lost(LostWhy::TimedOut))
            .await
            .unwrap();
        log.ack(&[1; 32], NOW).await.unwrap();
        // Acknowledged, and a payment names it again: the outcome is given again.
        assert_eq!(
            log.watch([1; 32], 500).await.unwrap(),
            Some(Outcome::Lost(LostWhy::TimedOut))
        );
        assert!(log.pending(NOW).await.unwrap().is_empty());
        log.watch([2; 32], 100).await.unwrap();
        log.settle(&[2; 32], Outcome::Confirmed).await.unwrap();
        assert_eq!(
            log.watch([2; 32], 500).await.unwrap(),
            Some(Outcome::Confirmed)
        );
        flusher.abort();
        drop(log);
        let (log, _f) = start(&d);
        assert_eq!(log.state(&[1; 32]), FundingState::Unknown);
        log.load().await.unwrap();
        assert_eq!(log.state(&[1; 32]), FundingState::Lost);
        assert_eq!(log.state(&[2; 32]), FundingState::Confirmed);
    }

    #[tokio::test]
    async fn sl_27_two_checks_at_once_give_each_outcome_once() {
        let d = disks("funding-race");
        let (log, _f) = start(&d);
        let log = Arc::new(log);
        log.watch([1; 32], 100).await.unwrap();
        let confirmed = [chan(1, Some(1), 1)];
        // The second sees the channel gone, as after a close just behind.
        let (a, b) = tokio::join!(log.check(&confirmed, 101, 1), log.check(&[], 101, 1));
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a.len() + b.len(), 1);
        let kept = log.pending(NOW).await.unwrap();
        assert_eq!(kept.len(), 1);
        let first = a.first().or(b.first()).unwrap();
        assert_eq!(&kept[0], first);
    }

    #[test]
    fn sl_27_a_channel_gone_at_the_claim_is_a_payment_through_a_lost_funding() {
        let channels = [chan(1, Some(11), 3)];
        // Not listed, its funding not known: flagged, and no funding named.
        let got = unconfirmed_fundings(&[[9; 32]], &channels, 1, |_| None);
        assert!(got.txids.is_empty() && got.lost_unknown && !got.more);
        // Not listed, but its funding was seen not confirmed: named.
        let known = |id: &[u8; 32]| (id == &[9; 32]).then_some(([99; 32], 0));
        let got = unconfirmed_fundings(&[[9; 32], [9; 32]], &channels, 1, known);
        assert_eq!((got.txids, got.lost_unknown), (ids(&[99]), false));
        // Seen with the depth: still confirmed, nothing is lost.
        let deep = |_: &[u8; 32]| Some(([99; 32], 6));
        let got = unconfirmed_fundings(&[[9; 32]], &channels, 6, deep);
        assert!(got.txids.is_empty() && !got.lost_unknown);
        // Not as deep as the shop asks: named.
        let got = unconfirmed_fundings(&[[9; 32]], &channels, 7, deep);
        assert_eq!(got.txids, ids(&[99]));
    }

    #[test]
    fn sl_27_every_pass_closes_the_listed_channels_of_a_lost_funding() {
        let channels = [
            chan(1, Some(11), 0),
            chan(2, Some(22), 0),
            chan(3, None, 0),
            chan(4, Some(44), 2),
        ];
        let lost = |t: &[u8; 32]| t == &[11; 32] || t == &[44; 32];
        assert_eq!(lost_channels(&channels, lost), vec![[1; 32], [4; 32]]);
        // Asked again with the channel still listed: the same, until it is gone.
        assert_eq!(lost_channels(&channels, lost), vec![[1; 32], [4; 32]]);
        assert!(lost_channels(&channels[1..3], lost).is_empty());
    }

    #[tokio::test]
    async fn sl_27_a_lost_funding_stays_lost_in_the_log_for_every_pass() {
        let d = disks("funding-lost-state");
        let (log, flusher) = start(&d);
        log.watch([1; 32], 100).await.unwrap();
        let channels = [chan(1, Some(1), 0)];
        log.check(&channels, 100 + LOST_AFTER_BLOCKS, 1)
            .await
            .unwrap();
        let lost = |t: &[u8; 32]| log.state(t) == FundingState::Lost;
        assert_eq!(lost_channels(&channels, lost), vec![[1; 32]]);
        // The next pass, and a start, still hold it lost.
        log.check(&channels, 100 + LOST_AFTER_BLOCKS + 1, 1)
            .await
            .unwrap();
        assert_eq!(lost_channels(&channels, lost), vec![[1; 32]]);
        flusher.abort();
        drop(log);
        let (log, _f) = start(&d);
        log.load().await.unwrap();
        let lost = |t: &[u8; 32]| log.state(t) == FundingState::Lost;
        assert_eq!(lost_channels(&channels, lost), vec![[1; 32]]);
    }

    #[tokio::test]
    async fn sl_27_a_funding_seen_in_a_block_is_confirmed_when_its_channel_closes_after_it() {
        let d = disks("funding-seen");
        let (log, _f) = start(&d);
        log.watch([1; 32], 100).await.unwrap();
        log.watch([2; 32], 100).await.unwrap();
        // Funding 1 is seen with one confirmation at 110; 2 is in no block.
        let channels = [chan(1, Some(1), 1), chan(2, Some(2), 0)];
        assert!(log.check(&channels, 110, 3).await.unwrap().is_empty());
        // Both channels close. The depth is 3: 1 waits until 112 (110 + 3 - 1),
        // 2 was never in a block.
        let got = log.check(&[], 111, 3).await.unwrap();
        assert_eq!(
            got,
            vec![FundingEvent {
                txid: [2; 32],
                outcome: Outcome::Lost(LostWhy::Closed)
            }]
        );
        assert_eq!(log.state(&[1; 32]), FundingState::Watching);
        let got = log.check(&[], 112, 3).await.unwrap();
        assert_eq!(
            got,
            vec![FundingEvent {
                txid: [1; 32],
                outcome: Outcome::Confirmed
            }]
        );
    }

    #[test]
    fn sl_27_a_funding_gone_with_its_channel_is_judged_by_where_it_was_seen() {
        let t = [22; 32];
        assert_eq!(judge(&t, 100, Some(105), 106, &[], 3), Outcome::Watching);
        assert_eq!(judge(&t, 100, Some(105), 107, &[], 3), Outcome::Confirmed);
        assert_eq!(
            judge(&t, 100, None, 107, &[], 3),
            Outcome::Lost(LostWhy::Closed)
        );
        assert_eq!(block_height(110, 1), Some(110));
        assert_eq!(block_height(110, 3), Some(108));
        assert_eq!(block_height(110, 0), None);
    }

    async fn seen_of(log: &FundingLog, t: u8) -> Option<u32> {
        log.read(&[t; 32]).await.unwrap().unwrap().seen_height
    }

    #[tokio::test]
    async fn sl_27_a_listed_funding_that_drops_to_no_confirmation_clears_its_seen_height() {
        let d = disks("funding-seen-cleared");
        let (log, _bg) = start(&d);
        log.watch([1; 32], 100).await.unwrap();
        // Seen with one confirmation at 110; depth 3.
        assert!(log
            .check(&[chan(1, Some(1), 1)], 110, 3)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(seen_of(&log, 1).await, Some(110));
        // A reorg removes the block: the channel is listed with none.
        assert!(log
            .check(&[chan(1, Some(1), 0)], 111, 3)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(seen_of(&log, 1).await, None);
        // The funding is beaten and the channel closes: a loss, however
        // deep the tip is.
        let got = log.check(&[], 130, 3).await.unwrap();
        assert_eq!(
            got,
            vec![FundingEvent {
                txid: [1; 32],
                outcome: Outcome::Lost(LostWhy::Closed)
            }]
        );
    }

    #[tokio::test]
    async fn sl_27_a_seen_height_moves_when_the_block_changes_in_a_reorg() {
        let d = disks("funding-seen-moves");
        let (log, _bg) = start(&d);
        log.watch([1; 32], 100).await.unwrap();
        log.check(&[chan(1, Some(1), 1)], 110, 4).await.unwrap();
        assert_eq!(seen_of(&log, 1).await, Some(110));
        // The funding is in another block, one higher.
        log.check(&[chan(1, Some(1), 1)], 111, 4).await.unwrap();
        assert_eq!(seen_of(&log, 1).await, Some(111));
        // The same block at the same height is not written again.
        log.check(&[chan(1, Some(1), 2)], 112, 4).await.unwrap();
        assert_eq!(seen_of(&log, 1).await, Some(111));
        // Not listed: the kept height stands.
        log.check(&[], 113, 4).await.unwrap();
        assert_eq!(seen_of(&log, 1).await, Some(111));
    }

    #[tokio::test]
    async fn sl_27_a_funding_in_a_block_at_the_claim_is_not_lost_by_a_close_before_the_check() {
        let d = disks("funding-seen-before-check");
        let (log, _bg) = start(&d);
        // At the claim the channel is open with one confirmation at the tip
        // 110: the funding is kept with that block before any check.
        log.watch_seen([1; 32], 110, Some(110)).await.unwrap();
        // A funding kept with no block takes one given later.
        log.watch([2; 32], 110).await.unwrap();
        log.watch_seen([2; 32], 110, Some(110)).await.unwrap();
        // Both channels close normally before the first check; depth 3.
        let got = log.check(&[], 112, 3).await.unwrap();
        assert_eq!(
            got,
            vec![
                FundingEvent {
                    txid: [1; 32],
                    outcome: Outcome::Confirmed
                },
                FundingEvent {
                    txid: [2; 32],
                    outcome: Outcome::Confirmed
                },
            ]
        );
    }

    #[tokio::test]
    async fn sl_27_a_channel_gone_at_the_claim_is_watched_by_where_it_was_last_seen() {
        let d = disks("funding-seen-at-claim");
        let (log, _bg) = start(&d);
        let last = |id: &[u8; 32]| match id[0] {
            // Last seen with one confirmation at the tip 110.
            1 => Some(([1; 32], 1, 110)),
            // Last seen with none.
            2 => Some(([2; 32], 0, 110)),
            _ => None,
        };
        let seen1 = last_seen_height(&[1; 32], &[[1; 32]], last);
        let seen2 = last_seen_height(&[2; 32], &[[2; 32]], last);
        assert_eq!(seen1, Some(110));
        assert_eq!(seen2, None);
        // Another funding of the same channel gives no height.
        assert_eq!(last_seen_height(&[3; 32], &[[1; 32]], last), None);
        log.watch_seen([1; 32], 115, seen1).await.unwrap();
        log.watch_seen([2; 32], 115, seen2).await.unwrap();
        // Depth 3: the funding seen at 110 is confirmed at 112; the other
        // was in no block and is lost.
        let got = log.check(&[], 115, 3).await.unwrap();
        assert_eq!(
            got,
            vec![
                FundingEvent {
                    txid: [1; 32],
                    outcome: Outcome::Confirmed
                },
                FundingEvent {
                    txid: [2; 32],
                    outcome: Outcome::Lost(LostWhy::Closed)
                }
            ]
        );
        // With the tip not yet deep enough, it is watched.
        log.watch_seen([3; 32], 111, Some(110)).await.unwrap();
        assert!(log.check(&[], 111, 3).await.unwrap().is_empty());
        assert_eq!(log.state(&[3; 32]), FundingState::Watching);
        let got = log.check(&[], 112, 3).await.unwrap();
        assert_eq!(
            got,
            vec![FundingEvent {
                txid: [3; 32],
                outcome: Outcome::Confirmed
            }]
        );
    }

    #[test]
    fn sl_27_the_blob_is_read_strictly() {
        let k = Kept {
            claimed_height: 700_000,
            seen_height: Some(699_999),
            outcome: Outcome::Lost(LostWhy::Closed),
            acked_unix: Some(9),
        };
        let good = encode(&k);
        assert_eq!(decode(&good).unwrap(), k);
        assert!(decode(&[]).is_err());
        assert!(decode(&good[..LEN - 1]).is_err());
        let mut long = good.clone();
        long.push(0);
        assert!(decode(&long).is_err());
        let mut b = good.clone();
        b[0] = 2;
        assert!(decode(&b).is_err());
        let mut b = good.clone();
        b[9] = 4;
        assert!(decode(&b).is_err());
        for flag in [2u8, 255] {
            let mut b = good.clone();
            b[10] = flag;
            assert!(decode(&b).is_err());
        }
        // Bytes behind a flag of 0.
        let mut b = encode(&Kept {
            acked_unix: None,
            ..k
        });
        assert!(decode(&b).is_ok());
        b[14] = 1;
        assert!(decode(&b).is_err());
        // An acknowledgment of a funding still watched.
        let mut b = good;
        b[9] = 0;
        assert!(decode(&b).is_err());
    }
}
