//! Chain data from the shop's own Bitcoin Core, over RPC, through
//! `lightning-block-sync` (SL-26). Never Electrum, Esplora or any server that
//! would learn the shop's addresses.
//!
//! Core gives whole blocks, so every output the library would register with
//! `chain::Filter` is seen without being asked for. The compact-filter watcher
//! of `docs/WATCHER-RULES.md` is the second source ([`crate::filters`]);
//! [`FilterWatcher`] is the seam it plugs into, and [`ChainSource`] says which
//! of the two a node is given.

use crate::filters::FilterConfig;
use crate::logger::Logger;
use bitcoin::consensus::encode::serialize_hex;
use bitcoin::{Script, Transaction, Txid};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use lightning::chain::{Filter, WatchedOutput};
use lightning::util::logger::Level;
use lightning_block_sync::http::HttpEndpoint;
use lightning_block_sync::rpc::RpcClient;
use openbmt_watch::FeeHistory;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Where the node's chain data comes from (SL-26): the shop's own Bitcoin
/// Core over RPC, or the compact-filter watcher on the counter's peer layer.
#[derive(Debug, Clone)]
pub enum ChainSource {
    /// The shop's Bitcoin Core: blocks, fee rates and sending transactions.
    Core(CoreRpc),
    /// The filter watcher: blocks come from peers whose filters matched, and
    /// transactions go out through the same peers. No fee source is behind it
    /// yet, so the fee rates stay at their fixed values.
    Filters(FilterConfig),
}

/// Where Bitcoin Core's RPC is, and how to log in.
#[derive(Debug, Clone)]
pub struct CoreRpc {
    /// Host, for instance `127.0.0.1`.
    pub host: String,
    /// RPC port.
    pub port: u16,
    /// RPC user.
    pub user: String,
    /// RPC password.
    pub password: String,
}

impl CoreRpc {
    pub(crate) fn client(&self) -> RpcClient {
        let credentials = base64(format!("{}:{}", self.user, self.password).as_bytes());
        RpcClient::new(
            &credentials,
            HttpEndpoint::for_host(self.host.clone()).with_port(self.port),
        )
    }
}

fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The seam for the compact-filter watcher ([`crate::filters::FilterQueue`]):
/// what it is told of the transactions and outputs the node wants to see. A
/// watcher that fetches only the blocks its filters match needs these;
/// Bitcoin Core's whole blocks do not.
pub trait FilterWatcher: Send + Sync {
    /// A transaction with this output script is of interest.
    fn register_tx(&self, txid: &Txid, script_pubkey: &Script);
    /// This output is of interest, to see its spend.
    fn register_output(&self, output: WatchedOutput);
}

/// What the library is told about chain data: a pass-through to the watcher
/// if there is one, nothing otherwise.
#[derive(Default)]
pub struct ChainFilter {
    watcher: RwLock<Option<Arc<dyn FilterWatcher>>>,
}

impl ChainFilter {
    /// Plugs in a compact-filter watcher.
    pub fn set_watcher(&self, watcher: Arc<dyn FilterWatcher>) {
        *self.watcher.write().unwrap() = Some(watcher);
    }
}

impl Filter for ChainFilter {
    fn register_tx(&self, txid: &Txid, script_pubkey: &Script) {
        if let Some(w) = self.watcher.read().unwrap().as_ref() {
            w.register_tx(txid, script_pubkey);
        }
    }

    fn register_output(&self, output: WatchedOutput) {
        if let Some(w) = self.watcher.read().unwrap().as_ref() {
            w.register_output(output);
        }
    }
}

/// Sends transactions through Core's `sendrawtransaction`, or, with the filter
/// watcher, through the peers the watcher is connected to.
pub struct CoreBroadcaster {
    via: Via,
    logger: Arc<Logger>,
}

enum Via {
    Core {
        client: Arc<RpcClient>,
        handle: tokio::runtime::Handle,
    },
    /// The raw transaction goes to the watcher's thread, which gives it to
    /// the driver.
    Peers(Mutex<Sender<Vec<u8>>>),
}

impl CoreBroadcaster {
    pub(crate) fn new(
        client: Arc<RpcClient>,
        handle: tokio::runtime::Handle,
        logger: Arc<Logger>,
    ) -> Self {
        Self {
            via: Via::Core { client, handle },
            logger,
        }
    }

    pub(crate) fn through_peers(sender: Sender<Vec<u8>>, logger: Arc<Logger>) -> Self {
        Self {
            via: Via::Peers(Mutex::new(sender)),
            logger,
        }
    }
}

impl BroadcasterInterface for CoreBroadcaster {
    fn broadcast_transactions(&self, txs: &[&Transaction]) {
        match &self.via {
            Via::Core { client, handle } => {
                for tx in txs {
                    let (client, logger) = (Arc::clone(client), Arc::clone(&self.logger));
                    let hex = serialize_hex(*tx);
                    let txid = tx.compute_txid();
                    handle.spawn(async move {
                        let params = [serde_json::json!(hex)];
                        let result = client
                            .call_method::<serde_json::Value>("sendrawtransaction", &params)
                            .await;
                        match result {
                            Ok(_) => logger.line(Level::Info, &format!("broadcast {txid}")),
                            // A transaction that is already known, or that double-spends one,
                            // is not a failure the library needs told about.
                            Err(e) => logger.line(Level::Debug, &format!("broadcast {txid}: {e}")),
                        }
                    });
                }
            }
            Via::Peers(sender) => {
                let sender = sender.lock().unwrap();
                for tx in txs {
                    let txid = tx.compute_txid();
                    match sender.send(bitcoin::consensus::encode::serialize(*tx)) {
                        Ok(()) => self
                            .logger
                            .line(Level::Info, &format!("broadcast {txid} to peers")),
                        Err(_) => self
                            .logger
                            .line(Level::Debug, &format!("broadcast {txid}: watcher stopped")),
                    }
                }
            }
        }
    }
}

/// Fee rates in satoshis per 1000 weight units, from Core's
/// `estimatesmartfee`, with fixed values where Core has none (regtest).
pub struct CoreFees {
    non_anchor_min: AtomicU32,
    normal: AtomicU32,
    urgent: AtomicU32,
    max_estimate: AtomicU32,
    /// With the filter watcher: the samples of blocks and the tip they are
    /// read at (SL-26a).
    history: RwLock<Option<(FeeHistory, u32)>>,
}

/// The floor the library enforces, in sat per 1000 weight units.
pub const FEE_FLOOR_SAT_PER_KW: u32 = 253;
const FEE_FALLBACK_NORMAL: u32 = 2_500;
const FEE_FALLBACK_URGENT: u32 = 5_000;

impl Default for CoreFees {
    fn default() -> Self {
        Self {
            non_anchor_min: AtomicU32::new(FEE_FLOOR_SAT_PER_KW),
            normal: AtomicU32::new(FEE_FALLBACK_NORMAL),
            urgent: AtomicU32::new(FEE_FALLBACK_URGENT),
            max_estimate: AtomicU32::new(FEE_FALLBACK_URGENT * 5),
            history: RwLock::new(None),
        }
    }
}

/// Satoshis per 1000 weight units from thousandths of a satoshi per virtual
/// byte, the unit of the fee history (`openbmt_watch::FeeSample`): a virtual
/// byte is 4 weight units, so a satoshi per virtual byte (1000 in the history)
/// is 250 per 1000 weight units, and the history's value divided by 4 is the
/// rate. Rounded up; never below the floor.
pub fn sat_per_kw_from_milli_per_vb(milli: u64) -> u32 {
    u32::try_from(milli.div_ceil(4))
        .unwrap_or(u32::MAX)
        .max(FEE_FLOOR_SAT_PER_KW)
}

impl CoreFees {
    /// Reads the rates from the samples of blocks, at this tip (SL-26a). A
    /// target the history cannot answer keeps the fixed rate.
    pub fn use_history(&self, history: FeeHistory, tip: u32) {
        *self.history.write().unwrap() = Some((history, tip));
    }

    /// What the history says for a target, if it says anything: the recent
    /// rate for the channel, the close and the spends (FE-13); twice the
    /// higher of the recent and the usual rate for the urgent rate and the
    /// most a provider may set (FE-17). The lowest a provider may set is not
    /// read from it.
    fn rate_from_history(&self, target: ConfirmationTarget) -> Option<u32> {
        let guard = self.history.read().unwrap();
        let (history, tip) = guard.as_ref()?;
        let recent = history.recent_rate(*tip);
        match target {
            ConfirmationTarget::MinAllowedAnchorChannelRemoteFee
            | ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee => None,
            ConfirmationTarget::UrgentOnChainSweep | ConfirmationTarget::MaximumFeeEstimate => {
                let high = recent.max(history.usual_rate(*tip))?;
                Some(sat_per_kw_from_milli_per_vb(high.saturating_mul(2)))
            }
            ConfirmationTarget::AnchorChannelFee
            | ConfirmationTarget::NonAnchorChannelFee
            | ConfirmationTarget::ChannelCloseMinimum
            | ConfirmationTarget::OutputSpendingFee => recent.map(sat_per_kw_from_milli_per_vb),
        }
    }

    /// Asks Core for estimates and keeps them; a target Core cannot estimate
    /// keeps its last value.
    pub async fn refresh(&self, client: &RpcClient) {
        async fn estimate(client: &RpcClient, blocks: u32) -> Option<u32> {
            let params = [serde_json::json!(blocks)];
            let r = client
                .call_method::<serde_json::Value>("estimatesmartfee", &params)
                .await
                .ok()?;
            // BTC per kvB to sat per kw: x 1e8 / 4.
            let per_kvb = r.get("feerate")?.as_f64()?;
            Some(((per_kvb * 100_000_000.0 / 4.0).round() as u32).max(FEE_FLOOR_SAT_PER_KW))
        }
        if let Some(v) = estimate(client, 6).await {
            self.normal.store(v, Ordering::Relaxed);
            self.non_anchor_min
                .store((v / 2).max(FEE_FLOOR_SAT_PER_KW), Ordering::Relaxed);
        }
        if let Some(v) = estimate(client, 2).await {
            self.urgent.store(v, Ordering::Relaxed);
            self.max_estimate
                .store(v.saturating_mul(5), Ordering::Relaxed);
        }
    }

    /// Keeps the estimates fresh until the task is dropped.
    pub async fn keep_fresh(self: Arc<Self>, client: Arc<RpcClient>) {
        loop {
            self.refresh(&client).await;
            tokio::time::sleep(Duration::from_secs(300)).await;
        }
    }
}

impl FeeEstimator for CoreFees {
    fn get_est_sat_per_1000_weight(&self, target: ConfirmationTarget) -> u32 {
        if let Some(rate) = self.rate_from_history(target) {
            return rate.max(FEE_FLOOR_SAT_PER_KW);
        }
        let v = match target {
            ConfirmationTarget::MaximumFeeEstimate => &self.max_estimate,
            ConfirmationTarget::UrgentOnChainSweep => &self.urgent,
            // The closing fee's lowest acceptable rate is the low one, so a
            // provider that estimates less can still agree to a close.
            ConfirmationTarget::MinAllowedAnchorChannelRemoteFee
            | ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee
            | ConfirmationTarget::ChannelCloseMinimum => &self.non_anchor_min,
            _ => &self.normal,
        };
        v.load(Ordering::Relaxed).max(FEE_FLOOR_SAT_PER_KW)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;
    use bitcoin::BlockHash;

    #[test]
    fn sl_26_credentials_are_base64_of_user_and_password() {
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64(b"a:b"), "YTpi");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"a"), "YQ==");
    }

    #[test]
    fn sl_26_fees_never_go_below_the_floor() {
        let fees = CoreFees::default();
        for t in [
            ConfirmationTarget::MaximumFeeEstimate,
            ConfirmationTarget::UrgentOnChainSweep,
            ConfirmationTarget::MinAllowedAnchorChannelRemoteFee,
            ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee,
            ConfirmationTarget::ChannelCloseMinimum,
        ] {
            assert!(fees.get_est_sat_per_1000_weight(t) >= FEE_FLOOR_SAT_PER_KW);
        }
    }

    use openbmt_watch::FeeSample;

    fn sample(height: u32, rate_milli: u64) -> FeeSample {
        FeeSample {
            height,
            block: BlockHash::from_byte_array([height as u8; 32]),
            fees_sats: 1,
            vsize: 1000,
            claimed_less: false,
            rate_milli: Some(rate_milli),
        }
    }

    fn history(samples: &[(u32, u64)]) -> FeeHistory {
        let mut h = FeeHistory::new();
        for (height, rate) in samples {
            h.insert(sample(*height, *rate));
        }
        h
    }

    fn rate(fees: &CoreFees, t: ConfirmationTarget) -> u32 {
        fees.get_est_sat_per_1000_weight(t)
    }

    #[test]
    fn sl_26a_the_history_unit_is_four_times_the_weight_unit() {
        // 1 sat/vB is 250 sat/kw, 4 sat/vB is 1000, 40 sat/vB is 10,000.
        assert_eq!(sat_per_kw_from_milli_per_vb(4_000), 1_000);
        assert_eq!(sat_per_kw_from_milli_per_vb(40_000), 10_000);
        assert_eq!(sat_per_kw_from_milli_per_vb(10_000), 2_500);
        // Rounded up: 4,001 thousandths is 1,000.25 sat/kw.
        assert_eq!(sat_per_kw_from_milli_per_vb(4_001), 1_001);
        // 1 sat/vB is under the floor of 253.
        assert_eq!(sat_per_kw_from_milli_per_vb(1_000), FEE_FLOOR_SAT_PER_KW);
        assert_eq!(sat_per_kw_from_milli_per_vb(u64::MAX), u32::MAX);
    }

    #[test]
    fn sl_26a_the_targets_read_the_recent_and_the_usual_rate() {
        let fees = CoreFees::default();
        // Three samples in the last 144 blocks: the recent rate is the median,
        // 12,000 thousandths, 3,000 sat/kw. No usual rate (fewer than 20).
        fees.use_history(history(&[(98, 8_000), (99, 12_000), (100, 16_000)]), 100);
        for t in [
            ConfirmationTarget::NonAnchorChannelFee,
            ConfirmationTarget::AnchorChannelFee,
            ConfirmationTarget::ChannelCloseMinimum,
            ConfirmationTarget::OutputSpendingFee,
        ] {
            assert_eq!(rate(&fees, t), 3_000, "{t:?}");
        }
        // Twice the higher of the recent and the usual rate: here the recent.
        assert_eq!(rate(&fees, ConfirmationTarget::UrgentOnChainSweep), 6_000);
        assert_eq!(rate(&fees, ConfirmationTarget::MaximumFeeEstimate), 6_000);
        // The lowest a provider may set stays at the floor.
        assert_eq!(
            rate(
                &fees,
                ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee
            ),
            FEE_FLOOR_SAT_PER_KW
        );
        assert_eq!(
            rate(&fees, ConfirmationTarget::MinAllowedAnchorChannelRemoteFee),
            FEE_FLOOR_SAT_PER_KW
        );
    }

    #[test]
    fn sl_26a_the_urgent_and_the_most_take_the_usual_rate_when_it_is_higher() {
        let fees = CoreFees::default();
        let mut samples: Vec<(u32, u64)> = (100..120).map(|h| (h, 40_000)).collect();
        samples.extend((3_900..3_906).map(|h| (h, 4_000)));
        fees.use_history(history(&samples), 3_905);
        // Recent: 4,000 (1,000 sat/kw). Usual: the 26 samples' lower median,
        // 40,000. The urgent rate is twice that, 80,000: 20,000 sat/kw.
        assert_eq!(rate(&fees, ConfirmationTarget::NonAnchorChannelFee), 1_000);
        assert_eq!(rate(&fees, ConfirmationTarget::UrgentOnChainSweep), 20_000);
        assert_eq!(rate(&fees, ConfirmationTarget::MaximumFeeEstimate), 20_000);
    }

    #[test]
    fn sl_26a_no_rate_is_below_the_floor() {
        let fees = CoreFees::default();
        fees.use_history(history(&[(98, 100), (99, 100), (100, 100)]), 100);
        for t in [
            ConfirmationTarget::MaximumFeeEstimate,
            ConfirmationTarget::UrgentOnChainSweep,
            ConfirmationTarget::MinAllowedAnchorChannelRemoteFee,
            ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee,
            ConfirmationTarget::AnchorChannelFee,
            ConfirmationTarget::NonAnchorChannelFee,
            ConfirmationTarget::ChannelCloseMinimum,
            ConfirmationTarget::OutputSpendingFee,
        ] {
            assert_eq!(rate(&fees, t), FEE_FLOOR_SAT_PER_KW, "{t:?}");
        }
    }

    #[test]
    fn sl_26a_with_no_samples_the_fixed_rates_are_used() {
        let plain = CoreFees::default();
        let empty = CoreFees::default();
        empty.use_history(FeeHistory::new(), 500);
        // Two samples are too few for a recent rate.
        let few = CoreFees::default();
        few.use_history(history(&[(99, 40_000), (100, 40_000)]), 100);
        for t in [
            ConfirmationTarget::MaximumFeeEstimate,
            ConfirmationTarget::UrgentOnChainSweep,
            ConfirmationTarget::NonAnchorChannelFee,
            ConfirmationTarget::ChannelCloseMinimum,
            ConfirmationTarget::OutputSpendingFee,
            ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee,
        ] {
            assert_eq!(rate(&empty, t), rate(&plain, t), "{t:?}");
            assert_eq!(rate(&few, t), rate(&plain, t), "{t:?}");
        }
        assert_eq!(rate(&plain, ConfirmationTarget::NonAnchorChannelFee), 2_500);
    }

    #[test]
    fn sl_26a_old_samples_leave_the_recent_window_as_the_tip_moves() {
        let fees = CoreFees::default();
        fees.use_history(history(&[(98, 8_000), (99, 12_000), (100, 16_000)]), 100);
        assert_eq!(rate(&fees, ConfirmationTarget::NonAnchorChannelFee), 3_000);
        // 144 blocks later the samples are outside the window.
        fees.use_history(history(&[(98, 8_000), (99, 12_000), (100, 16_000)]), 250);
        assert_eq!(rate(&fees, ConfirmationTarget::NonAnchorChannelFee), 2_500);
    }
}
