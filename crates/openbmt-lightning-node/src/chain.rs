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
        }
    }
}

impl CoreFees {
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
}
