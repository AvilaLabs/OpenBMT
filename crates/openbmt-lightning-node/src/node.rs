//! The node: the library wired as the rules say.
//!
//! - Receive only (SL-01): no public way to pay or forward, and channels are
//!   opened by the provider alone.
//! - The seed is the 64 bytes of SL-10; the library never takes the words.
//! - State on two devices (SL-20 to SL-25), through `store.rs`.
//! - Chain data from the shop's own Bitcoin Core (SL-26), `chain.rs`, or from
//!   the compact-filter watcher, `filters.rs`.
//! - Channels from the one provider the shop picked, by LSPS2 (SL-28 to
//!   SL-31), `provider.rs`.
//! - A sale is paid only on `Event::PaymentClaimed` (SL-21): the library gives
//!   that event only after the monitor update that holds the preimage is
//!   complete, and an update is complete only when both copies are written.
//!   The paid event is kept in the store until the caller acknowledges it
//!   (`paid.rs`), and what is expected of each invoice survives a restart
//!   (`expect.rs`).
//! - Spendable outputs are swept to an address the node derives from the shop's
//!   pinned wallet, at an index the caller gives (`sweep.rs`).

use crate::bip85::NodeSeed;
use crate::chain::{ChainFilter, ChainSource, CoreBroadcaster, CoreFees};
use crate::device::{DeviceSnapshot, DeviceState, DeviceStatus, SecondDevice};
use crate::expect::{Claim, Expectation, Expectations};
use crate::filters::{Confirmable, FilterQueue, FilterRun, FilterStart, Follower, Progress};
use crate::funding::{self, ChannelFunding, FundingEvent, FundingLog};
use crate::locks::MutexExt;
use crate::logger::Logger;
use crate::lsps1::{self, OrderBook, OrderRecord, OrderState, OrderWhy, Orders};
use crate::paid::{PaidLog, PaidRecord};
use crate::provider::{choose_offer, Offer, ProviderConfig, Refusal};
use crate::room::{decide, room_to_receive_msat, OnChainReason, ReceiveDecision, Situation};
use crate::seal::SealKey;
use crate::state::{self, StartFrom, StateError};
use crate::store::{Background, DeviceView, DualStore};
use crate::sweep::{SweepDestination, Sweeper, SweeperParts};
use crate::types::*;
use bitcoin::bip32::Xpriv;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::{PublicKey, Secp256k1};
use bitcoin::{Address, BlockHash, Network};
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::{BestBlock, Listen, Watch};
use lightning::events::{Event, PaymentPurpose, ReplayEvent};
use lightning::ln::channelmanager::{
    Bolt11InvoiceParameters, ChainParameters, ChannelManagerReadArgs, PaymentId,
    RecentPaymentDetails, RecipientOnionFields, RetryableSendFailure,
};
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
use lightning::onion_message::messenger::NullMessageRouter;
use lightning::routing::router::{Path, Route, RouteHint, RouteHintHop, RouteHop};
use lightning::sign::{InMemorySigner, KeysManager, NodeSigner};
use lightning::types::features::{ChannelFeatures, NodeFeatures};
use lightning::types::payment::{PaymentHash, PaymentSecret};
use lightning::util::config::{
    ChannelConfigOverrides, ChannelConfigUpdate, ChannelHandshakeConfigUpdate, UserConfig,
};
use lightning::util::logger::Level;
use lightning::util::persist::{
    KVStore, CHANNEL_MANAGER_PERSISTENCE_KEY, CHANNEL_MANAGER_PERSISTENCE_PRIMARY_NAMESPACE,
    CHANNEL_MANAGER_PERSISTENCE_SECONDARY_NAMESPACE,
};
use lightning::util::ser::ReadableArgs;
use lightning_block_sync::init::{synchronize_listeners, validate_best_block_header};
use lightning_block_sync::poll::ChainPoller;
use lightning_block_sync::rpc::RpcClient;
use lightning_block_sync::{SpvClient, UnboundedCache};
use lightning_invoice::{
    Bolt11Invoice, Bolt11InvoiceDescription, Currency, Description, InvoiceBuilder,
};
use lightning_liquidity::events::LiquidityEvent;
use lightning_liquidity::lsps0::ser::LSPSRequestId;
use lightning_liquidity::lsps1::client::LSPS1ClientConfig;
use lightning_liquidity::lsps1::event::LSPS1ClientEvent;
use lightning_liquidity::lsps1::msgs::{
    LSPS1ChannelInfo, LSPS1Options, LSPS1OrderId, LSPS1OrderParams, LSPS1PaymentInfo,
};
use lightning_liquidity::lsps2::client::LSPS2ClientConfig;
use lightning_liquidity::lsps2::event::LSPS2ClientEvent;
use lightning_liquidity::lsps2::msgs::LSPS2OpeningFeeParams;
use lightning_liquidity::LiquidityClientConfig;
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_watch::HeaderStore;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Notify};

/// A monitor with what it needs to be brought to the tip.
type MonitorListener = (
    ChannelMonitor<InMemorySigner>,
    Arc<CoreBroadcaster>,
    Arc<CoreFees>,
    Arc<Logger>,
);

/// Whether Shop Lightning may run on `Network::Bitcoin`. Off until SL-60 is
/// met: the library's async persistence out of beta or a review of its
/// funds-loss cases, the purist check, and a run on signet.
pub const MAINNET_ALLOWED: bool = false;

/// The most a pending update count the persister keeps before it writes the
/// whole monitor again.
const MAX_PENDING_UPDATES: u64 = 100;
/// How long to wait for the provider to answer an LSPS2 request.
const PROVIDER_ANSWER: Duration = Duration::from_secs(15);
/// How often the chain tip is polled.
const POLL_EVERY: Duration = Duration::from_secs(1);
/// A channel's funding transaction, its confirmations when last seen and the
/// tip height they were read at.
type LastFunding = ([u8; 32], u32, u32);
/// How often the fundings watched for the counter are judged (SL-27).
const FUNDING_CHECK_EVERY: Duration = Duration::from_secs(5);
/// A chain poll older than this counts as the chain data being behind (SL-41).
const CHAIN_STALE_SECS: u64 = 60;
/// How long an invoice lives, in seconds.
const INVOICE_EXPIRY_SECS: u32 = 3600;
/// The minimum final CLTV of an invoice that goes through an LSPS2 provider
/// (two more than the library's default, as bLIP-52 asks).
const JIT_MIN_FINAL_CLTV: u16 = 80 + 2;

/// The words of the key a start needs: the 64-byte seed (SL-10), where
/// everything is kept, and who the shop's provider, device and Core are.
pub struct NodeConfig {
    /// The network. `Network::Bitcoin` is refused (SL-60).
    pub network: Network,
    /// The node's seed.
    pub seed: NodeSeed,
    /// The counter's own directory for the node.
    pub data_dir: PathBuf,
    /// Where the chain data comes from (SL-26): the shop's Bitcoin Core or the
    /// compact-filter watcher.
    pub chain: ChainSource,
    /// With Tor on, the SOCKS5 proxy of Tor. The connection to the provider
    /// and the watcher's peers then go through it, and a proxy that cannot be
    /// reached stops the node: nothing is sent over the clear net instead
    /// (SL-32).
    pub tor: Option<SocketAddr>,
    /// The second device (SL-20).
    pub device: Arc<dyn SecondDevice>,
    /// The provider (SL-28).
    pub provider: ProviderConfig,
    /// The most the shop pays for one opening, in millisatoshis (SL-29).
    pub max_opening_fee_msat: u64,
    /// The cap, in satoshis (SL-34): room is bought ahead (SL-28) when the room
    /// to receive is under half of it. 0 buys none.
    pub cap_sat: u64,
    /// Which copy of the state to start from (SL-25).
    pub start_from: StartFrom,
    /// The shop's pinned wallet description, as the counter pins it (WL-20,
    /// WL-22). The node derives the addresses the sweep, the splice out and
    /// the close pay to from it (SL-33); an address is never handed in. One
    /// that does not read is refused (`words::SL_33_DESTINATION_REFUSED`).
    pub wallet: String,
    /// The receive index the sweeper pays to on a first start. After the
    /// caller has changed it with [`Node::set_sweep_index`] the changed one is
    /// kept and used.
    pub sweep_index: u32,
    /// The confirmations a funding needs to count as confirmed (SL-40): the
    /// counter's own, at least 1.
    pub confirm_depth: u32,
}

/// Why the node did not start.
#[derive(Debug)]
pub enum StartError {
    /// The network is `Bitcoin`, which is refused until SL-60 (`SL_60`).
    MainnetNotYet,
    /// The state is not safe to start from (SL-25).
    State(StateError),
    /// The second device cannot be reached to compare the copies (SL-23).
    DeviceAway(String),
    /// Bitcoin Core is not the network asked for, or cannot be read.
    Chain(String),
    /// A disk, the keys or the library's own error.
    Setup(String),
    /// Tor is on and its proxy cannot be reached (SL-32). Nothing is
    /// connected, and nothing is made on disk.
    TorUnreachable(String),
    /// The filter watcher could not be started (SL-26).
    Filters(String),
    /// The shop's wallet description does not read, or the sweep index has no
    /// address in it (SL-33).
    WalletRefused,
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::MainnetNotYet => f.write_str(crate::words::SL_60),
            StartError::State(StateError::OlderCopy(_)) => f.write_str(crate::words::SL_25),
            StartError::State(StateError::Diverged) => f.write_str(crate::words::SL_25_DIVERGED),
            StartError::DeviceAway(e) => write!(f, "{}: {e}", crate::words::SL_23),
            StartError::WalletRefused => f.write_str(crate::words::SL_33_DESTINATION_REFUSED),
            StartError::TorUnreachable(e) => {
                write!(f, "{}: {e}", crate::words::SL_32_TOR_UNREACHABLE)
            }
            StartError::Filters(e) => write!(f, "{}: {e}", crate::words::SL_26_FILTERS_NOT_STARTED),
            StartError::Chain(e) | StartError::Setup(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for StartError {}

fn setup(e: impl std::fmt::Display) -> StartError {
    StartError::Setup(e.to_string())
}

/// A sale that is paid (SL-21): the preimage is claimed and the state that
/// holds the claim is on both devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paid {
    /// The payment hash of the invoice.
    pub payment_hash: [u8; 32],
    /// What arrived, in millisatoshis: the amount of the invoice less the
    /// provider's fee if the payment opened a channel.
    pub amount_msat: u64,
    /// What the provider skimmed off this payment, in millisatoshis: 0 when
    /// no channel was opened for it. The customer paid `amount_msat` plus
    /// this.
    pub skimmed_msat: u64,
    /// What the payer sent, if the library knows it.
    pub sender_intended_msat: Option<u64>,
    /// When the claim was seen, in seconds since the epoch. A repeat of the
    /// event carries the time of the first.
    pub received_unix: u64,
    /// The funding transaction of every channel the payment came through
    /// whose funding had not the shop's confirmations when it was claimed
    /// (SL-40), at most [`funding::MAX_FUNDINGS`]. The node then tells of
    /// the confirmation of each or the loss of any
    /// ([`Node::try_funding`]).
    pub unconfirmed_fundings: Vec<[u8; 32]>,
    /// More such channels than are listed: the payment is never final at
    /// once.
    pub more_fundings: bool,
    /// An HTLC of the payment came through a channel that was no longer
    /// listed at the claim and whose funding the node did not know (SL-27):
    /// the payment is one through a lost funding.
    pub lost_unknown: bool,
}

impl From<PaidRecord> for Paid {
    fn from(r: PaidRecord) -> Self {
        Paid {
            payment_hash: r.payment_hash,
            amount_msat: r.amount_msat,
            skimmed_msat: r.skimmed_msat,
            sender_intended_msat: r.sender_intended_msat,
            received_unix: r.received_unix,
            unconfirmed_fundings: r.unconfirmed_fundings,
            more_fundings: r.more_fundings,
            lost_unknown: r.lost_unknown,
        }
    }
}

/// What the counter is told for an amount (SL-29, SL-40, SL-41).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receive {
    /// On-chain code alone.
    OnChainOnly(OnChainReason),
    /// An invoice on the channels that are open.
    Invoice,
    /// An invoice that makes the provider open a channel, at this fee, shown
    /// to the customer's amount before the invoice is made (SL-29).
    NewChannel {
        /// The provider's fee for the opening, in millisatoshis.
        fee_msat: u64,
        /// The offer the invoice will be made from.
        offer: Offer,
    },
}

/// An invoice for the counter, and what the opening costs if it opens one.
#[derive(Debug, Clone)]
pub struct MadeInvoice {
    /// The invoice.
    pub invoice: Bolt11Invoice,
    /// The provider's fee, if the payment opens a channel.
    pub opening_fee_msat: Option<u64>,
}

/// Why an invoice was not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvoiceError {
    /// The decision was on-chain only.
    OnChainOnly(OnChainReason),
    /// The provider did not answer, or refused.
    Provider(String),
    /// The invoice's expectations could not be stored on both devices, so the
    /// invoice is not handed back (SL-20).
    NotStored,
    /// The library could not make it.
    Library(String),
}

/// What the manager reads (SL-23, SL-45).
#[derive(Debug, Clone)]
pub struct NodeStatus {
    /// The second device.
    pub device: DeviceSnapshot,
    /// Whether the chain data is up to date (a poll of Core in the last minute).
    pub chain_ok: bool,
    /// The height the node has reached.
    pub chain_height: u32,
    /// Whether the provider is connected.
    pub provider_connected: bool,
    /// Channels of any state.
    pub channels: usize,
    /// Channels that can take a payment now.
    pub usable_channels: usize,
    /// The room to receive (SL-40), in millisatoshis.
    pub room_msat: u64,
    /// Whether the provider offers LSPS1 (SL-28).
    pub lsps1: lsps1::Offer,
    /// The last order for room bought ahead (SL-45).
    pub last_order: Option<OrderRecord>,
    /// What orders paid so far, in satoshis (SL-29, SL-45).
    pub lsps1_fees_paid_sat: u64,
    /// A paid order brought no channel, so no more room is bought from this
    /// provider (SL-28).
    pub lsps1_stopped: bool,
}

/// What the provider says of an order (SL-28).
struct OrderAnswer {
    id: String,
    /// The order as the provider approved it.
    params: LSPS1OrderParams,
    payment: LSPS1PaymentInfo,
    channel: Option<LSPS1ChannelInfo>,
}

/// Waits for the answer to the request `id`; if none comes in time, the
/// waiter's entry is taken out of the map, so an answer that never comes
/// leaves nothing behind.
async fn answer_within<T>(
    waiters: &Mutex<HashMap<LSPSRequestId, Waiter>>,
    id: &LSPSRequestId,
    rx: oneshot::Receiver<T>,
    wait: Duration,
) -> Option<T> {
    let answer = tokio::time::timeout(wait, rx).await;
    if answer.is_err() {
        waiters.lock_recover().remove(id);
    }
    answer.ok()?.ok()
}

enum Waiter {
    Params(oneshot::Sender<Vec<LSPS2OpeningFeeParams>>),
    Buy(oneshot::Sender<(u64, u32)>),
    Options(oneshot::Sender<LSPS1Options>),
    Order(oneshot::Sender<OrderAnswer>),
}

/// What the node knows of the provider's LSPS1 (SL-28).
struct Lsps1State {
    offer: lsps1::Offer,
    options: Option<LSPS1Options>,
    /// When an open order was last asked about, in seconds since the epoch.
    last_poll: u64,
}

struct Shared {
    network: Network,
    provider: ProviderConfig,
    max_opening_fee_msat: u64,
    cap_sat: u64,
    manager: Arc<Manager>,
    monitor: Arc<Monitor>,
    peers: Arc<Peers>,
    liquidity: Arc<Liquidity>,
    keys: Arc<KeysManager>,
    fees: Arc<CoreFees>,
    store: Arc<DualStore>,
    background: Arc<Background>,
    device_status: Arc<DeviceStatus>,
    logger: Arc<Logger>,
    paid: mpsc::UnboundedSender<Paid>,
    funding: mpsc::UnboundedSender<FundingEvent>,
    waiters: Mutex<HashMap<LSPSRequestId, Waiter>>,
    expectations: Expectations,
    paid_log: PaidLog,
    orders: Orders,
    lsps1: Mutex<Lsps1State>,
    funding_log: FundingLog,
    /// The funding, its confirmations and the tip height they were read at,
    /// last seen for each channel, to name the funding of a channel that
    /// closed before the claim (SL-27). Kept in memory only; a channel not
    /// here is a funding not known.
    last_fundings: Mutex<HashMap<[u8; 32], LastFunding>>,
    confirm_depth: u32,
    sweeper: Arc<Sweeper>,
    destination: Arc<SweepDestination>,
    tor: Option<SocketAddr>,
    chain_height: AtomicU64,
    last_poll_ok: AtomicU64,
    stopping: AtomicBool,
    stop: Notify,
}

/// The funding facts needed before the node releases an inbound payment
/// preimage. This is deliberately separate from the later paid-event funding
/// log: that log is for cashier policy, while this gate protects settlement.
#[derive(Debug, Clone, Copy)]
struct ClaimChannelFunding {
    channel_id: [u8; 32],
    confirmations: Option<u32>,
    has_funding_output: bool,
}

/// Whether every channel carrying a claimable payment has funding that LDK
/// reports confirmed to the shop's configured depth. An empty/incomplete
/// channel list, unknown channel, or unavailable expected output fails closed.
fn claim_funding_ready(
    receiving_channels: &[[u8; 32]],
    channels: &[ClaimChannelFunding],
    depth: u32,
) -> bool {
    let depth = depth.max(1);
    !receiving_channels.is_empty()
        && receiving_channels.iter().all(|channel_id| {
            channels.iter().any(|channel| {
                &channel.channel_id == channel_id
                    && channel.has_funding_output
                    && channel.confirmations.unwrap_or(0) >= depth
            })
        })
}

/// What the chain poll tells of each block: the monitors, the manager and the
/// sweeper, in that order.
struct Listeners {
    monitor: Arc<Monitor>,
    manager: Arc<Manager>,
    sweeper: Arc<Sweeper>,
}

impl Listen for Listeners {
    fn filtered_block_connected(
        &self,
        header: &bitcoin::block::Header,
        txdata: &lightning::chain::transaction::TransactionData,
        height: u32,
    ) {
        self.monitor
            .filtered_block_connected(header, txdata, height);
        self.manager
            .filtered_block_connected(header, txdata, height);
        self.sweeper
            .filtered_block_connected(header, txdata, height);
    }

    fn blocks_disconnected(&self, fork_point: BestBlock) {
        self.monitor.blocks_disconnected(fork_point);
        self.manager.blocks_disconnected(fork_point);
        self.sweeper.blocks_disconnected(fork_point);
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The shop's Lightning node, running.
pub struct Node {
    shared: Arc<Shared>,
    paid_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Paid>>,
    node_id: PublicKey,
    funding_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<FundingEvent>>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    filters: Mutex<Option<FilterRun>>,
    processor: Mutex<Option<tokio::task::JoinHandle<Result<(), lightning::io::Error>>>>,
}

/// The configuration of the library for this node: unannounced channels,
/// accepted by hand, no forwarding (SL-01, SL-30, SL-31).
pub fn user_config() -> UserConfig {
    let mut c = UserConfig::default();
    c.channel_handshake_config.announce_for_forwarding = false;
    c.channel_handshake_config.our_to_self_delay = crate::provider::WANTED_TO_SELF_DELAY as u16;
    c.channel_handshake_config
        .negotiate_anchors_zero_fee_htlc_tx = false;
    // A shutdown script is chosen at the close, to the shop's wallet (SL-36).
    c.channel_handshake_config.commit_upfront_shutdown_pubkey = false;
    c.channel_handshake_limits
        .force_announced_channel_preference = true;
    c.channel_handshake_limits.max_funding_satoshis = 100_000_000;
    c.manually_accept_inbound_channels = true;
    c.accept_forwards_to_priv_channels = false;
    c.accept_intercept_htlcs = false;
    c.reject_inbound_splices = false;
    // The provider takes its fee out of a payment on any channel it has with
    // the shop, not only the one that was opened for it: every channel the
    // node accepts is the provider's (SL-31), so every one takes underpaying
    // HTLCs (SL-29). What the node claims is still judged by `expect.rs`.
    c.channel_config.accept_underpaying_htlcs = true;
    c
}

/// Sets `accept_underpaying_htlcs` on every channel with the provider that
/// is already open. A channel made now has it from [`user_config`] and from
/// the accept; one made before it, or by an older build, gets it here, so a
/// payment that makes the provider open another channel can be forwarded over
/// it less the provider's fee.
fn accept_underpaying_on_open_channels(manager: &Manager, provider: &PublicKey, logger: &Logger) {
    let ids: Vec<_> = manager
        .list_channels()
        .into_iter()
        .filter(|c| {
            c.counterparty.node_id == *provider
                && !c.config.is_some_and(|c| c.accept_underpaying_htlcs)
        })
        .map(|c| c.channel_id)
        .collect();
    if ids.is_empty() {
        return;
    }
    let update = ChannelConfigUpdate {
        accept_underpaying_htlcs: Some(true),
        ..Default::default()
    };
    let result = manager.update_partial_channel_config(provider, &ids, &update);
    logger.line(
        Level::Info,
        &format!(
            "underpaying HTLCs accepted on {} open channel(s): {result:?}",
            ids.len()
        ),
    );
}

fn network_name_ok(network: Network, chain: &str) -> bool {
    matches!(
        (network, chain),
        (Network::Regtest, "regtest")
            | (Network::Signet, "signet")
            | (Network::Testnet, "test")
            | (Network::Bitcoin, "main")
    )
}

impl Node {
    /// Starts the node. Refuses `Network::Bitcoin` (SL-60), compares the two
    /// copies of the state and refuses an older one (SL-25), and brings the
    /// chain data to the tip of the shop's Bitcoin Core.
    pub async fn start(config: NodeConfig) -> Result<Node, StartError> {
        if config.network == Network::Bitcoin && !MAINNET_ALLOWED {
            return Err(StartError::MainnetNotYet);
        }
        // The shop's wallet (SL-33), before anything is made on disk.
        let wallet =
            PinnedWallet::from_text(&config.wallet).map_err(|_| StartError::WalletRefused)?;
        wallet
            .receive_script(config.sweep_index)
            .map_err(|_| StartError::WalletRefused)?;
        // Tor is on: a proxy that cannot be reached is a stop, before anything
        // is made or connected (SL-32).
        if let Some(proxy) = config.tor {
            tor_reachable(proxy)
                .await
                .map_err(StartError::TorUnreachable)?;
        }
        let handle = tokio::runtime::Handle::current();
        std::fs::create_dir_all(&config.data_dir).map_err(setup)?;
        let logger =
            Arc::new(Logger::open(&config.data_dir.join("node.log"), Level::Debug).map_err(setup)?);
        let status = Arc::new(DeviceStatus::new());

        // Keys (SL-10): the seed is the master key of the node's own tree.
        let master = Xpriv::new_master(config.network, &config.seed.0).map_err(setup)?;
        let seed32 = master.private_key.secret_bytes();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(setup)?;
        let keys = Arc::new(KeysManager::new(
            &seed32,
            now.as_secs(),
            now.subsec_nanos(),
            true,
        ));
        let node_id = keys
            .get_node_id(lightning::sign::Recipient::Node)
            .map_err(|()| StartError::Setup("the node's own key".to_string()))?;
        let seal_key = SealKey::from_seed(&config.seed);

        // Chain data (SL-26).
        let fees = Arc::new(CoreFees::default());
        let filter = Arc::new(ChainFilter::default());
        let filter_queue = Arc::new(FilterQueue::default());
        let (rpc, broadcaster, broadcasts) = match &config.chain {
            ChainSource::Core(core) => {
                let rpc = Arc::new(core.client());
                let chain_name = rpc_chain(&rpc).await.map_err(StartError::Chain)?;
                if !network_name_ok(config.network, &chain_name) {
                    return Err(StartError::Chain(format!("core is on {chain_name}")));
                }
                fees.refresh(&rpc).await;
                let broadcaster = Arc::new(CoreBroadcaster::new(
                    Arc::clone(&rpc),
                    handle.clone(),
                    Arc::clone(&logger),
                ));
                (Some(rpc), broadcaster, None)
            }
            ChainSource::Filters(_) => {
                // The library registers what it wants to see in a queue the
                // watcher reads (SL-27); transactions go out through the peers.
                filter.set_watcher(Arc::clone(&filter_queue) as _);
                let (sender, receiver) = std::sync::mpsc::channel();
                let broadcaster =
                    Arc::new(CoreBroadcaster::through_peers(sender, Arc::clone(&logger)));
                (None, broadcaster, Some(receiver))
            }
        };

        // The two copies (SL-20, SL-25).
        let state_dir = config.data_dir.join("state");
        std::fs::create_dir_all(&state_dir).map_err(setup)?;
        let local = Arc::new(lightning_persister::fs_store::FilesystemStore::new(
            state_dir.clone(),
        ));
        let view = DeviceView::new(
            Arc::clone(&config.device),
            seal_key.clone(),
            Arc::clone(&keys),
        );
        let env = state::ReadEnv {
            keys: Arc::clone(&keys),
            broadcaster: Arc::clone(&broadcaster),
            fees: Arc::clone(&fees),
            logger: Arc::clone(&logger),
        };
        let device_numbers = state::update_numbers(Arc::new(view.clone()), &env)
            .await
            .map_err(|e| StartError::DeviceAway(e.to_string()))?;
        let local_numbers = state::update_numbers(Arc::clone(&local), &env)
            .await
            .map_err(setup)?;
        let found = state::order(&local_numbers, &device_numbers);
        logger.line(Level::Info, &format!("copies: {found:?}"));
        let mut pulled = false;
        match state::action(found, config.start_from).map_err(StartError::State)? {
            state::Action::Go => {}
            state::Action::PushToDevice => {
                state::push(&state_dir, &*local, &view)
                    .await
                    .map_err(setup)?;
            }
            state::Action::PullFromDevice => {
                state::pull(&state_dir, &*local, &view)
                    .await
                    .map_err(setup)?;
                pulled = true;
            }
        }

        let store = Arc::new(DualStore::new(
            Arc::clone(&local),
            Arc::clone(&config.device),
            seal_key,
            Arc::clone(&keys),
            Arc::clone(&status),
            Arc::clone(&logger),
        ));
        let background = Arc::new(Background(Arc::clone(&store)));
        let flusher = store.spawn_lazy_flusher(&handle);
        if pulled {
            // What the counter kept of the background writes goes back to
            // the device, which may be behind it.
            state::owe_lazy(&state_dir, &background)
                .await
                .map_err(setup)?;
        }
        let now_unix = now_secs();
        let expectations = Expectations::load(Arc::clone(&store), now_unix)
            .await
            .map_err(setup)?;
        let orders = Orders::load(Arc::clone(&store)).await.map_err(setup)?;
        let paid_log = PaidLog::new(Arc::clone(&background));
        let funding_log = FundingLog::new(Arc::clone(&background));
        let destination = Arc::new(
            SweepDestination::load(
                Arc::clone(&background),
                wallet,
                config.network,
                config.sweep_index,
            )
            .await
            .map_err(setup)?,
        );
        let sweeper_parts = SweeperParts {
            broadcaster: Arc::clone(&broadcaster),
            fees: Arc::clone(&fees),
            filter: Arc::clone(&filter),
            store: Arc::clone(&background),
            logger: Arc::clone(&logger),
            keys: Arc::clone(&keys),
            destination: Arc::clone(&destination),
        };
        let kept_sweeper = sweeper_parts.read().await.map_err(setup)?;
        let persister = |store: &Arc<DualStore>| -> Persister {
            Persister::new(
                Arc::clone(store),
                TokioSpawner(handle.clone()),
                Arc::clone(&logger),
                MAX_PENDING_UPDATES,
                Arc::clone(&keys),
                Arc::clone(&keys),
                Arc::clone(&broadcaster),
                Arc::clone(&fees),
            )
        };
        let monitors = persister(&store)
            .read_all_channel_monitors_with_updates()
            .await
            .map_err(setup)?;

        let monitor: Arc<Monitor> = Arc::new(ChainMonitor::new_async_beta(
            Some(Arc::clone(&filter)),
            Arc::clone(&broadcaster),
            Arc::clone(&logger),
            Arc::clone(&fees),
            persister(&store),
            Arc::clone(&keys),
            keys.get_peer_storage_key(),
        ));
        let router = Arc::new(NoRouter);
        let message_router = Arc::new(NullMessageRouter {});

        // The channel manager, from the counter's copy (now the newer one).
        let manager_bytes = match KVStore::read(
            &*store,
            CHANNEL_MANAGER_PERSISTENCE_PRIMARY_NAMESPACE,
            CHANNEL_MANAGER_PERSISTENCE_SECONDARY_NAMESPACE,
            CHANNEL_MANAGER_PERSISTENCE_KEY,
        )
        .await
        {
            Ok(b) => Some(b),
            Err(e) if e.kind() == lightning::io::ErrorKind::NotFound => None,
            Err(e) => return Err(setup(e)),
        };
        let mut cache = UnboundedCache::new();
        // The manager, where the library has reached (`start_best`), the tip
        // of Core's chain when Core is the source, the monitors, and the lowest
        // height any part of the library has reached.
        let (manager, start_best, core_tip, monitors, floor) = match manager_bytes {
            Some(bytes) => {
                let refs: Vec<&ChannelMonitor<InMemorySigner>> =
                    monitors.iter().map(|(_, m)| m).collect();
                let read_args = ChannelManagerReadArgs::new(
                    Arc::clone(&keys),
                    Arc::clone(&keys),
                    Arc::clone(&keys),
                    Arc::clone(&fees),
                    Arc::clone(&monitor),
                    Arc::clone(&broadcaster),
                    Arc::clone(&router),
                    Arc::clone(&message_router),
                    Arc::clone(&logger),
                    user_config(),
                    refs,
                );
                let (manager_hash, manager) =
                    <(BlockHash, Manager)>::read(&mut lightning::io::Cursor::new(bytes), read_args)
                        .map_err(|e| setup(format!("manager: {e:?}")))?;
                match &rpc {
                    Some(rpc) => {
                        // Bring the manager and each monitor to the tip, together.
                        let listeners: Vec<(BlockHash, MonitorListener)> = monitors
                            .into_iter()
                            .map(|(hash, m)| {
                                (
                                    hash,
                                    (
                                        m,
                                        Arc::clone(&broadcaster),
                                        Arc::clone(&fees),
                                        Arc::clone(&logger),
                                    ),
                                )
                            })
                            .collect();
                        let mut set: Vec<(BlockHash, &dyn Listen)> =
                            vec![(manager_hash, &manager as &dyn Listen)];
                        if let Some((best, sweeper)) = &kept_sweeper {
                            set.push((best.block_hash, &**sweeper as &dyn Listen));
                        }
                        for (hash, l) in &listeners {
                            set.push((*hash, l as &dyn Listen));
                        }
                        let tip =
                            synchronize_listeners(Arc::clone(rpc), config.network, &mut cache, set)
                                .await
                                .map_err(|e| StartError::Chain(format!("{e:?}")))?;
                        (
                            manager,
                            tip.to_best_block(),
                            Some(tip),
                            listeners.into_iter().map(|(_, l)| l.0).collect::<Vec<_>>(),
                            tip.height,
                        )
                    }
                    None => {
                        // The watcher brings each part from where it was.
                        let best = manager.current_best_block();
                        let floor = monitors
                            .iter()
                            .map(|(_, m)| m.current_best_block().height)
                            .chain(kept_sweeper.as_ref().map(|(b, _)| b.height))
                            .fold(best.height, u32::min);
                        (
                            manager,
                            best,
                            None,
                            monitors.into_iter().map(|(_, m)| m).collect(),
                            floor,
                        )
                    }
                }
            }
            None => {
                let (tip, core_tip, start_floor) = match &rpc {
                    Some(rpc) => {
                        let tip = match &kept_sweeper {
                            // Spendable outputs are tracked: the sweeper is brought to
                            // the tip, the manager is new there.
                            Some((best, sweeper)) => synchronize_listeners(
                                Arc::clone(rpc),
                                config.network,
                                &mut cache,
                                vec![(best.block_hash, &**sweeper as &dyn Listen)],
                            )
                            .await
                            .map_err(|e| StartError::Chain(format!("{e:?}")))?,
                            None => validate_best_block_header(Arc::clone(rpc))
                                .await
                                .map_err(|e| StartError::Chain(format!("{e:?}")))?,
                        };
                        (tip.to_best_block(), Some(tip), tip.height)
                    }
                    None => {
                        // A new manager starts at the first block the watcher's
                        // header store trusts (WR-04), and the watcher feeds it
                        // forward from there.
                        let store = HeaderStore::new(config.network)
                            .map_err(|e| StartError::Filters(e.to_string()))?;
                        let best = BestBlock::new(store.start_hash(), store.start_height());
                        let floor = kept_sweeper
                            .as_ref()
                            .map_or(best.height, |(b, _)| b.height.min(best.height));
                        (best, None, floor)
                    }
                };
                let manager = Manager::new(
                    Arc::clone(&fees),
                    Arc::clone(&monitor),
                    Arc::clone(&broadcaster),
                    Arc::clone(&router),
                    Arc::clone(&message_router),
                    Arc::clone(&logger),
                    Arc::clone(&keys),
                    Arc::clone(&keys),
                    Arc::clone(&keys),
                    user_config(),
                    ChainParameters {
                        network: config.network,
                        best_block: tip,
                    },
                    now.as_secs() as u32,
                );
                (
                    manager,
                    tip,
                    core_tip,
                    monitors.into_iter().map(|(_, m)| m).collect(),
                    start_floor,
                )
            }
        };
        let manager = Arc::new(manager);
        let sweeper = match kept_sweeper {
            Some((_, sweeper)) => sweeper,
            None => sweeper_parts.new_at(start_best),
        };
        for m in monitors {
            let id = m.channel_id();
            monitor
                .watch_channel(id, m)
                .map_err(|_| setup("watch_channel"))?;
        }

        // The LSPS2 client and the peers.
        let liquidity = Arc::new(
            Liquidity::new(
                Arc::clone(&keys),
                Arc::clone(&keys),
                Arc::clone(&manager),
                Some(Arc::clone(&filter)),
                Some(ChainParameters {
                    network: config.network,
                    best_block: start_best,
                }),
                Arc::clone(&background),
                Arc::clone(&broadcaster),
                None,
                Some(LiquidityClientConfig {
                    lsps1_client_config: Some(LSPS1ClientConfig {
                        max_channel_fees_msat: Some(config.max_opening_fee_msat),
                    }),
                    lsps2_client_config: Some(LSPS2ClientConfig {}),
                    lsps5_client_config: None,
                }),
            )
            .await
            .map_err(setup)?,
        );
        let handlers = MessageHandler {
            chan_handler: Arc::clone(&manager),
            route_handler: Arc::new(IgnoringMessageHandler {}),
            onion_message_handler: Arc::new(IgnoringMessageHandler {}),
            custom_message_handler: Arc::clone(&liquidity),
            send_only_message_handler: Arc::clone(&monitor),
        };
        let ephemeral: [u8; 32] = lightning::sign::EntropySource::get_secure_random_bytes(&*keys);
        let peers: Arc<Peers> = Arc::new(Peers::new(
            handlers,
            now.as_secs() as u32,
            &ephemeral,
            Arc::clone(&logger),
            Arc::clone(&keys),
        ));

        let (paid_tx, paid_rx) = mpsc::unbounded_channel();
        funding_log.load().await.map_err(setup)?;
        let mut re_emitted: Vec<FundingEvent> = Vec::new();
        // The paid events the caller has not acknowledged come again, with the
        // same payment hash.
        for record in paid_log.pending(now_unix).await.map_err(setup)? {
            // A payment through a channel not yet confirmed is watched
            // again, in case the stop came between the two writes (SL-40).
            for txid in &record.unconfirmed_fundings {
                if let Some(outcome) = funding_log
                    .watch(*txid, start_best.height)
                    .await
                    .map_err(setup)?
                {
                    re_emitted.push(FundingEvent {
                        txid: *txid,
                        outcome,
                    });
                }
            }
            let _ = paid_tx.send(Paid::from(record));
        }
        let (funding_tx, funding_rx) = mpsc::unbounded_channel();
        for event in re_emitted {
            let _ = funding_tx.send(event);
        }
        for event in funding_log.pending(now_unix).await.map_err(setup)? {
            let _ = funding_tx.send(event);
        }
        let shared = Arc::new(Shared {
            network: config.network,
            provider: config.provider.clone(),
            max_opening_fee_msat: config.max_opening_fee_msat,
            cap_sat: config.cap_sat,
            manager: Arc::clone(&manager),
            monitor: Arc::clone(&monitor),
            peers: Arc::clone(&peers),
            liquidity: Arc::clone(&liquidity),
            keys: Arc::clone(&keys),
            fees: Arc::clone(&fees),
            store: Arc::clone(&store),
            background: Arc::clone(&background),
            device_status: Arc::clone(&status),
            logger: Arc::clone(&logger),
            paid: paid_tx,
            funding: funding_tx,
            waiters: Mutex::new(HashMap::new()),
            expectations,
            paid_log,
            orders,
            lsps1: Mutex::new(Lsps1State {
                offer: lsps1::Offer::Unknown,
                options: None,
                last_poll: 0,
            }),
            funding_log,
            last_fundings: Mutex::new(HashMap::new()),
            confirm_depth: config.confirm_depth.max(1),
            sweeper: Arc::clone(&sweeper),
            destination: Arc::clone(&destination),
            tor: config.tor,
            chain_height: AtomicU64::new(start_best.height as u64),
            // The watcher says when it is up to date; Core is polled at once.
            last_poll_ok: AtomicU64::new(if rpc.is_some() { now_secs() } else { 0 }),
            stopping: AtomicBool::new(false),
            stop: Notify::new(),
        });

        // Channels the provider opened before the setting was made take
        // underpaying HTLCs too (SL-29, SL-31).
        accept_underpaying_on_open_channels(&manager, &config.provider.node_id, &logger);

        let mut tasks = vec![flusher];

        // Chain: Core's tip polled and fed to the monitor and the manager, or
        // the compact-filter watcher following the driver (SL-26).
        let mut filter_run = None;
        match (&config.chain, rpc, core_tip) {
            (ChainSource::Filters(filters), _, _) => {
                let mut filters = filters.clone();
                if let Some(proxy) = config.tor {
                    filters.driver.proxy = Some(proxy);
                }
                let confirmables: Vec<Confirmable> = vec![
                    Arc::clone(&manager) as _,
                    Arc::clone(&monitor) as _,
                    Arc::clone(&sweeper) as _,
                ];
                let follower = Follower::new(confirmables, Arc::clone(&filter_queue), floor);
                let progress_shared = Arc::clone(&shared);
                let log = Arc::clone(&logger);
                let run = FilterRun::start(FilterStart {
                    config: filters,
                    network: config.network,
                    follower,
                    broadcasts: broadcasts.ok_or_else(|| setup("no broadcast queue"))?,
                    fees: Arc::clone(&fees),
                    data_dir: config.data_dir.clone(),
                    progress: Arc::new(move |p: Progress| {
                        progress_shared
                            .chain_height
                            .store(p.height as u64, Ordering::SeqCst);
                        if p.in_sync {
                            progress_shared
                                .last_poll_ok
                                .store(now_secs(), Ordering::SeqCst);
                        }
                    }),
                    log: Arc::new(move |line: &str| log.line(Level::Info, line)),
                })
                .map_err(StartError::Filters)?;
                filter_run = Some(run);
            }
            (ChainSource::Core(_), Some(rpc), Some(chain_tip)) => {
                let (shared, rpc, fees, network) = (
                    Arc::clone(&shared),
                    Arc::clone(&rpc),
                    Arc::clone(&fees),
                    config.network,
                );
                let listener = Listeners {
                    monitor: Arc::clone(&monitor),
                    manager: Arc::clone(&manager),
                    sweeper: Arc::clone(&sweeper),
                };
                tasks.push(tokio::spawn(async move {
                    let poller = ChainPoller::new(Arc::clone(&rpc), network);
                    let mut spv = SpvClient::new(chain_tip, poller, &mut cache, &listener);
                    let mut ticks = 0u64;
                    loop {
                        tokio::select! {
                            _ = shared.stop.notified() => return,
                            _ = tokio::time::sleep(POLL_EVERY) => {}
                        }
                        match spv.poll_best_tip().await {
                            Ok((_, changed)) => {
                                shared.last_poll_ok.store(now_secs(), Ordering::SeqCst);
                                if changed {
                                    let h = listener.manager.current_best_block().height;
                                    shared.chain_height.store(h as u64, Ordering::SeqCst);
                                }
                            }
                            Err(e) => shared
                                .logger
                                .line(Level::Warn, &format!("chain poll: {e:?}")),
                        }
                        ticks += 1;
                        if ticks.is_multiple_of(300) {
                            fees.refresh(&rpc).await;
                        }
                    }
                }));
            }
            _ => return Err(setup("chain source")),
        }

        // The fundings of the channels a payment came through before their
        // funding had a confirmation (SL-27, SL-40).
        {
            let shared = Arc::clone(&shared);
            tasks.push(tokio::spawn(async move { watch_fundings(shared).await }));
        }

        // The provider: stay connected to it, and only to it (SL-31).
        {
            let shared = Arc::clone(&shared);
            tasks.push(tokio::spawn(async move {
                keep_provider_connected(shared).await
            }));
        }
        // The LSPS1 and LSPS2 answers.
        {
            let shared = Arc::clone(&shared);
            tasks.push(tokio::spawn(async move { lsps2_events(shared).await }));
        }
        // Room bought ahead from a provider that offers LSPS1 (SL-28).
        {
            let shared = Arc::clone(&shared);
            tasks.push(tokio::spawn(async move { lsps1_task(shared).await }));
        }

        // The library's own loop: events, timers, persistence.
        let processor = {
            let shared = Arc::clone(&shared);
            let handler_shared = Arc::clone(&shared);
            tokio::spawn(async move {
                let stop_shared = Arc::clone(&shared);
                lightning_background_processor::process_events_async(
                    Arc::clone(&shared.background),
                    move |event| {
                        let s = Arc::clone(&handler_shared);
                        async move { handle_event(s, event).await }
                    },
                    Arc::clone(&shared.monitor),
                    Arc::clone(&shared.manager),
                    None::<Arc<Onion>>,
                    NoGossip::None,
                    Arc::clone(&shared.peers),
                    Some(Arc::clone(&shared.liquidity)),
                    Some(Arc::clone(&shared.sweeper)),
                    Arc::clone(&shared.logger),
                    None::<Arc<crate::sweep::NoScorer>>,
                    move |d: Duration| {
                        let s = Arc::clone(&stop_shared);
                        Box::pin(async move {
                            tokio::select! {
                                _ = s.stop.notified() => true,
                                _ = tokio::time::sleep(d) => false,
                            }
                        })
                    },
                    false,
                    || {
                        Some(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default(),
                        )
                    },
                )
                .await
            })
        };

        logger.line(Level::Info, &format!("started, node id {node_id}"));
        Ok(Node {
            shared,
            node_id,
            paid_rx: tokio::sync::Mutex::new(paid_rx),
            funding_rx: tokio::sync::Mutex::new(funding_rx),
            tasks: Mutex::new(tasks),
            filters: Mutex::new(filter_run),
            processor: Mutex::new(Some(processor)),
        })
    }

    /// The node's public key.
    pub fn node_id(&self) -> PublicKey {
        self.node_id
    }

    /// The next sale that is paid (SL-21). Events are given at least once: each
    /// is kept until [`Node::ack_paid`], and a start gives again every one not
    /// acknowledged, with the same payment hash. The counter treats a repeat
    /// as the same payment.
    pub async fn next_paid(&self) -> Option<Paid> {
        self.paid_rx.lock().await.recv().await
    }

    /// A sale that is paid, if one is waiting.
    pub fn try_paid(&self) -> Option<Paid> {
        self.paid_rx.try_lock().ok()?.try_recv().ok()
    }

    /// What became of the funding of a channel a paid sale came through
    /// before the funding had a confirmation (SL-27, SL-40): its first
    /// confirmation, or its loss. Events are given at least once, as paid
    /// ones are: each is kept until [`Node::ack_funding`], and a start gives
    /// again every one not acknowledged.
    pub fn try_funding(&self) -> Option<FundingEvent> {
        self.funding_rx.try_lock().ok()?.try_recv().ok()
    }

    /// What the node holds of a funding named in a paid event (SL-27,
    /// SL-40): watched, confirmed, lost or not known. It is known after the
    /// event of its outcome has been acknowledged, so that a sale is never
    /// left waiting for an event that was given before.
    pub fn funding_state(&self, txid: &[u8; 32]) -> funding::FundingState {
        self.shared.funding_log.state(txid)
    }

    /// The caller has the event of this funding: it is not given again.
    pub async fn ack_funding(&self, txid: [u8; 32]) -> Result<(), String> {
        self.shared
            .funding_log
            .ack(&txid, now_secs())
            .await
            .map_err(|e| e.to_string())
    }

    /// The caller has marked the sale with this payment hash paid: its event
    /// is not given again after a restart (SL-21). Acknowledging one that is
    /// not kept is not an error.
    pub async fn ack_paid(&self, payment_hash: [u8; 32]) -> Result<(), String> {
        self.shared
            .paid_log
            .ack(&payment_hash, now_secs())
            .await
            .map_err(|e| e.to_string())
    }

    /// The invoice with this payment hash is cancelled: the node no longer
    /// claims a payment to it, and one that arrives is failed back to the
    /// payer. Cancelling one that is not kept is not an error.
    pub async fn cancel_invoice(&self, payment_hash: [u8; 32]) -> Result<(), String> {
        self.shared
            .expectations
            .forget(&payment_hash, now_secs())
            .await
            .map_err(|e| e.to_string())
    }

    /// Changes where swept outputs go: the receive address of the shop's
    /// pinned wallet at `index`, derived by the node (SL-33). The index is
    /// kept, and used after a restart. An index with no address is refused
    /// with `words::SL_33_DESTINATION_REFUSED`.
    pub async fn set_sweep_index(&self, index: u32) -> Result<(), String> {
        self.shared
            .destination
            .set(index)
            .await
            .map_err(|e| match e.kind() {
                lightning::io::ErrorKind::InvalidInput => {
                    crate::words::SL_33_DESTINATION_REFUSED.to_string()
                }
                _ => e.to_string(),
            })
    }

    /// The index swept outputs go to now.
    pub fn sweep_index(&self) -> u32 {
        self.shared.destination.index()
    }

    /// The address swept outputs go to now.
    pub fn sweep_destination(&self) -> Result<Address, String> {
        let d = &self.shared.destination;
        d.address_at(d.index()).map_err(|e| e.to_string())
    }

    /// The shop wallet's receive address at `index`, the one the node derives.
    pub fn wallet_address(&self, index: u32) -> Result<Address, String> {
        self.shared
            .destination
            .address_at(index)
            .map_err(|_| crate::words::SL_33_DESTINATION_REFUSED.to_string())
    }

    /// How many spendable outputs the sweeper still tracks: those not yet
    /// spent, and those spent but not yet buried deep enough to forget.
    pub fn tracked_outputs(&self) -> usize {
        self.shared.sweeper.tracked_spendable_outputs().len()
    }

    /// The transactions the sweeper has made to spend outputs and not yet
    /// forgotten, as transaction IDs. A new one is a transaction that paid
    /// the sweep address, so the caller moves on to a new address.
    pub fn sweeper_spends(&self) -> Vec<[u8; 32]> {
        use bitcoin::hashes::Hash;
        use lightning::util::sweep::OutputSpendStatus;
        let mut out: Vec<[u8; 32]> = self
            .shared
            .sweeper
            .tracked_spendable_outputs()
            .into_iter()
            .filter_map(|o| match o.status {
                OutputSpendStatus::PendingInitialBroadcast { .. } => None,
                OutputSpendStatus::PendingFirstConfirmation {
                    latest_spending_tx, ..
                }
                | OutputSpendStatus::PendingThresholdConfirmations {
                    latest_spending_tx, ..
                } => Some(latest_spending_tx.compute_txid().to_byte_array()),
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// How many balances the channel monitors still hold: money that is
    /// claimable on the chain or waits for a confirmation. A closed channel
    /// whose money is back in the shop's wallet has none (SL-77).
    pub fn claimable_balances(&self) -> usize {
        self.shared.monitor.get_claimable_balances(&[]).len()
    }

    /// What the manager reads.
    pub fn status(&self) -> NodeStatus {
        let s = &self.shared;
        let channels = s.manager.list_channels();
        let usable = s.manager.list_usable_channels();
        let book: OrderBook = s.orders.snapshot();
        let stopped = book.is_stopped(&s.provider.node_id.serialize());
        NodeStatus {
            device: s.device_status.snapshot(),
            chain_ok: s.chain_ok(),
            chain_height: s.chain_height.load(Ordering::SeqCst) as u32,
            provider_connected: s.peers.peer_by_node_id(&s.provider.node_id).is_some(),
            channels: channels.len(),
            usable_channels: usable.len(),
            room_msat: room_to_receive_msat(usable.iter().map(|c| c.inbound_capacity_msat)),
            lsps1: s.lsps1.lock_recover().offer,
            last_order: book.order,
            lsps1_fees_paid_sat: book.fees_paid_sat,
            lsps1_stopped: stopped,
        }
    }

    /// The second device's status (SL-23).
    pub fn second_device_status(&self) -> DeviceSnapshot {
        self.shared.device_status.snapshot()
    }

    /// The room to receive now, in millisatoshis (SL-40).
    pub fn room_to_receive_msat(&self) -> u64 {
        room_to_receive_msat(
            self.shared
                .manager
                .list_usable_channels()
                .iter()
                .map(|c| c.inbound_capacity_msat),
        )
    }

    /// What the counter does for `amount_msat`: an invoice on the open
    /// channels, one that opens a channel, or the on-chain code alone
    /// (SL-29, SL-40, SL-41).
    pub async fn quote(&self, amount_msat: u64) -> Receive {
        let s = &self.shared;
        let situation = Situation {
            room_msat: self.room_to_receive_msat(),
            device_ok: s.device_status.snapshot().state == DeviceState::Reachable,
            chain_ok: s.chain_ok(),
            provider_can_open: true,
        };
        match decide(amount_msat, &situation) {
            ReceiveDecision::OnChainOnly(r) => Receive::OnChainOnly(r),
            ReceiveDecision::Invoice => Receive::Invoice,
            ReceiveDecision::NewChannel => {
                if s.peers.peer_by_node_id(&s.provider.node_id).is_none() {
                    return Receive::OnChainOnly(OnChainReason::ProviderAway);
                }
                let menu = match s.opening_params().await {
                    Ok(m) => m,
                    Err(_) => return Receive::OnChainOnly(OnChainReason::ProviderAway),
                };
                match choose_offer(
                    &menu,
                    amount_msat,
                    s.max_opening_fee_msat,
                    now_secs() as i64,
                ) {
                    Ok(offer) => Receive::NewChannel {
                        fee_msat: offer.fee_msat,
                        offer,
                    },
                    Err(Refusal::FeeAboveMax { .. }) => {
                        Receive::OnChainOnly(OnChainReason::FeeAboveMax)
                    }
                    Err(Refusal::WindowTooShort) => {
                        Receive::OnChainOnly(OnChainReason::WindowTooShort)
                    }
                    Err(_) => Receive::OnChainOnly(OnChainReason::NoRoom),
                }
            }
        }
    }

    /// Makes the invoice a quote gave. `description` is the words on the invoice.
    pub async fn invoice(
        &self,
        amount_msat: u64,
        quote: &Receive,
        description: &str,
    ) -> Result<MadeInvoice, InvoiceError> {
        let s = &self.shared;
        let description = Description::new(description.to_string())
            .map_err(|_| InvoiceError::Library("description".into()))?;
        match quote {
            Receive::OnChainOnly(r) => Err(InvoiceError::OnChainOnly(*r)),
            Receive::Invoice => {
                let invoice = s
                    .manager
                    .create_bolt11_invoice(Bolt11InvoiceParameters {
                        amount_msats: Some(amount_msat),
                        description: Bolt11InvoiceDescription::Direct(description),
                        invoice_expiry_delta_secs: Some(INVOICE_EXPIRY_SECS),
                        ..Default::default()
                    })
                    .map_err(|e| InvoiceError::Library(format!("{e:?}")))?;
                s.keep_expectation(invoice.payment_hash().to_byte_array(), 0)
                    .await?;
                Ok(MadeInvoice {
                    invoice,
                    opening_fee_msat: None,
                })
            }
            Receive::NewChannel { fee_msat, offer } => {
                let (scid, cltv_delta) = s.buy(amount_msat, offer).await?;
                let invoice = s.jit_invoice(amount_msat, description, scid, cltv_delta)?;
                s.keep_expectation(invoice.payment_hash().to_byte_array(), *fee_msat)
                    .await?;
                Ok(MadeInvoice {
                    invoice,
                    opening_fee_msat: Some(*fee_msat),
                })
            }
        }
    }

    /// Hands the node a connection to the provider that something else made:
    /// the Tor stream of SL-32, already connected. No Tor code is here.
    pub fn connect_provider_over(&self, stream: std::net::TcpStream) {
        let peers = Arc::clone(&self.shared.peers);
        let id = self.shared.provider.node_id;
        tokio::spawn(lightning_net_tokio::setup_outbound(peers, id, stream));
    }

    /// The confirmations of each channel's funding transaction as the library
    /// counts them from the chain data it was given (SL-26); `None` while it is
    /// not confirmed.
    pub fn funding_confirmations(&self) -> Vec<Option<u32>> {
        self.shared
            .manager
            .list_channels()
            .iter()
            .map(|c| c.confirmations)
            .collect()
    }

    /// What the node holds and could move out now, in satoshis: the spendable
    /// side of every channel.
    pub fn balance_sat(&self) -> u64 {
        self.shared
            .manager
            .list_channels()
            .iter()
            .map(|c| c.outbound_capacity_msat / 1000)
            .sum()
    }

    /// Moves `amount_sat` out of the channel with the most to give, to `to`,
    /// by a splice out (SL-33), to the pinned wallet's receive address at
    /// `index`, which the node derives itself; it never takes an address from
    /// another device. The splice is begun; it is done when the new funding
    /// transaction is locked by both sides.
    pub fn sweep(&self, index: u32, amount_sat: u64) -> Result<(), String> {
        let s = &self.shared;
        let to = self.wallet_address(index)?;
        let channel = s
            .manager
            .list_usable_channels()
            .into_iter()
            .filter(|c| c.outbound_capacity_msat >= amount_sat * 1000)
            .max_by_key(|c| c.outbound_capacity_msat)
            .ok_or_else(|| "no channel holds that much".to_string())?;
        let feerate = lightning::chain::chaininterface::FeeEstimator::get_est_sat_per_1000_weight(
            &*s.fees,
            lightning::chain::chaininterface::ConfirmationTarget::NonAnchorChannelFee,
        );
        let output = bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(amount_sat),
            script_pubkey: to.script_pubkey(),
        };
        s.manager
            .splice_channel(
                &channel.channel_id,
                &channel.counterparty.node_id,
                lightning::ln::funding::SpliceContribution::SpliceOut {
                    outputs: vec![output],
                },
                feerate,
                None,
            )
            .map_err(|e| format!("{e:?}"))
    }

    /// Closes every channel cooperatively to the pinned wallet's receive
    /// address at `index`, which the node derives itself (SL-36). It is
    /// refused while a splice is not locked yet; a channel already closing
    /// is left, and one that refuses does not stop the others.
    pub fn close_all(&self, index: u32) -> Result<crate::close::Closing, String> {
        let to = self.wallet_address(index)?;
        crate::close::close_all(
            &self.shared.manager,
            &self.shared.monitor,
            &self.shared.logger,
            &to,
        )
    }

    /// The ids of the channels the node lists, open or closing.
    pub fn channel_ids(&self) -> Vec<[u8; 32]> {
        self.shared
            .manager
            .list_channels()
            .iter()
            .map(|c| c.channel_id.0)
            .collect()
    }

    /// Closes by force the listed channels among `ids` (SL-36): the latest
    /// commitment is broadcast. Refused while a splice is not locked yet.
    pub fn force_close(&self, ids: &[[u8; 32]]) -> Result<crate::close::Forced, String> {
        let ids: Vec<_> = ids
            .iter()
            .map(|b| lightning::ln::types::ChannelId::from_bytes(*b))
            .collect();
        crate::close::force_close(
            &self.shared.manager,
            &self.shared.monitor,
            &self.shared.logger,
            &ids,
        )
    }

    /// Stops the node: the library's loop writes the state a last time, and
    /// the peers are let go.
    pub async fn stop(&self) {
        // Writes of the monitors still on their way finish first, so that the
        // manager written at the end is not older than a monitor (a stale
        // manager makes the library close the channel on the next start).
        for _ in 0..200 {
            if self.shared.device_status.snapshot().pending_writes == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.shared.stop.notify_waiters();
        // `notify_waiters` wakes only those waiting now; keep telling until
        // the loops are done.
        let tasks: Vec<_> = std::mem::take(&mut *self.tasks.lock_recover());
        let processor = self.processor.lock_recover().take();
        for t in tasks {
            let _ = tokio::time::timeout(Duration::from_secs(5), t).await;
        }
        if let Some(p) = processor {
            let mut p = p;
            for _ in 0..50 {
                self.shared.stop.notify_waiters();
                if tokio::time::timeout(Duration::from_millis(200), &mut p)
                    .await
                    .is_ok()
                {
                    break;
                }
            }
        }
        // The watcher's thread, and the driver with it, which saves its headers.
        let filters = self.filters.lock_recover().take();
        if let Some(mut run) = filters {
            let _ = tokio::task::spawn_blocking(move || run.stop()).await;
        }
        // What the background owes the second device, once more.
        let _ =
            tokio::time::timeout(Duration::from_secs(5), self.shared.store.flush_lazy_once()).await;
        self.shared.peers.disconnect_all_peers();
    }
}

impl Shared {
    /// SL-27: each funding watched for the counter is judged against the
    /// channels the library lists now. One that got its first confirmation,
    /// or is lost, is kept as an event and given to the counter.
    async fn check_fundings(&self) -> Result<(), lightning::io::Error> {
        let details = self.manager.list_channels();
        let channels: Vec<ChannelFunding> = details.iter().map(ChannelFunding::of).collect();
        let height = self.manager.current_best_block().height;
        self.note_fundings(&channels, height);
        for event in self
            .funding_log
            .check(&channels, height, self.confirm_depth)
            .await?
        {
            self.logger
                .line(Level::Info, &format!("funding {:?}", event.outcome));
            let _ = self.funding.send(event);
        }
        // A funding lost leaves its channel open, and the provider may still
        // send through it: every pass closes each channel listed whose
        // funding is lost, so that a close that failed, or was cut short by
        // a stop, is tried again and no more payments come through it
        // (SL-27).
        let lost = funding::lost_channels(&channels, |t| {
            self.funding_log.state(t) == funding::FundingState::Lost
        });
        for id in lost {
            let Some(c) = details.iter().find(|c| c.channel_id.0 == id) else {
                continue;
            };
            if let Err(e) = self.manager.force_close_broadcasting_latest_txn(
                &c.channel_id,
                &c.counterparty.node_id,
                "its funding is lost".to_string(),
            ) {
                self.logger.line(
                    Level::Warn,
                    &format!(
                        "channel {} of a lost funding not closed: {e:?}",
                        c.channel_id
                    ),
                );
            }
        }
        Ok(())
    }

    /// Keeps the funding and confirmations of each channel listed, for the
    /// claim of a payment through a channel that has closed since (SL-27).
    fn note_fundings(&self, channels: &[ChannelFunding], height: u32) {
        /// More channels than a node has in its life: the rest are not kept.
        const MAX_KEPT: usize = 4096;
        let mut last = self.last_fundings.lock_recover();
        for c in channels {
            let Some(txid) = c.funding_txid else { continue };
            if last.len() < MAX_KEPT || last.contains_key(&c.channel_id) {
                last.insert(c.channel_id, (txid, c.confirmations, height));
            }
        }
    }

    fn chain_ok(&self) -> bool {
        now_secs().saturating_sub(self.last_poll_ok.load(Ordering::SeqCst)) < CHAIN_STALE_SECS
    }

    /// Asks the provider for its offers (SL-28).
    async fn opening_params(&self) -> Result<Vec<LSPS2OpeningFeeParams>, ()> {
        let handler = self.liquidity.lsps2_client_handler().ok_or(())?;
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut waiters = self.waiters.lock_recover();
            let id =
                handler.request_opening_params(self.provider.node_id, self.provider.token.clone());
            waiters.insert(id.clone(), Waiter::Params(tx));
            id
        };
        answer_within(&self.waiters, &id, rx, PROVIDER_ANSWER)
            .await
            .ok_or(())
    }

    /// Asks the provider for its LSPS1 options. No answer in 30 seconds or an
    /// error means it has no LSPS1; that is kept and asked again at the next
    /// connection (SL-28).
    async fn ask_lsps1_options(&self) {
        let answer = async {
            let handler = self.liquidity.lsps1_client_handler()?;
            let (tx, rx) = oneshot::channel();
            let id = {
                let mut waiters = self.waiters.lock_recover();
                let id = handler.request_supported_options(self.provider.node_id);
                waiters.insert(id.clone(), Waiter::Options(tx));
                id
            };
            answer_within(&self.waiters, &id, rx, lsps1::OPTIONS_ANSWER).await
        }
        .await;
        let mut st = self.lsps1.lock_recover();
        match answer {
            Some(options) => {
                st.offer = lsps1::Offer::Offered;
                st.options = Some(options);
                self.logger.line(Level::Info, "the provider offers LSPS1");
            }
            None => {
                st.offer = lsps1::Offer::NotOffered;
                st.options = None;
                self.logger.line(
                    Level::Info,
                    "the provider has no LSPS1: room comes with a payment too big for it",
                );
            }
        }
    }

    /// Sends one order request and waits for the provider's answer.
    async fn order_request(
        &self,
        send: impl FnOnce(
            &lightning_liquidity::lsps1::client::LSPS1ClientHandler<
                Arc<KeysManager>,
                Arc<Background>,
            >,
        ) -> LSPSRequestId,
    ) -> Option<OrderAnswer> {
        let handler = self.liquidity.lsps1_client_handler()?;
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut waiters = self.waiters.lock_recover();
            let id = send(handler);
            waiters.insert(id.clone(), Waiter::Order(tx));
            id
        };
        answer_within(&self.waiters, &id, rx, lsps1::ORDER_ANSWER).await
    }

    /// The most one usable channel to the provider can send now, in
    /// millisatoshis.
    fn best_outbound_msat(&self) -> u64 {
        self.manager
            .list_usable_channels()
            .iter()
            .filter(|c| c.counterparty.node_id == self.provider.node_id)
            .map(|c| c.next_outbound_htlc_limit_msat)
            .max()
            .unwrap_or(0)
    }

    /// One step of room bought ahead (SL-28): follow the open order whatever
    /// the provider's offer is, or make one if the room is under half the cap
    /// and the provider's options are known (`options`).
    async fn lsps1_step(&self, options: Option<&LSPS1Options>) {
        let now = now_secs();
        // Nothing is ordered or paid while the copies cannot both be written
        // or the chain data is behind: the order would not be kept.
        if !self.chain_ok() || self.device_status.snapshot().state != DeviceState::Reachable {
            return;
        }
        let book = self.orders.snapshot();
        match book.order.as_ref().filter(|o| o.state.is_open()) {
            Some(record) => {
                if now.saturating_sub(self.lsps1.lock_recover().last_poll) < lsps1::POLL_EVERY_SECS
                {
                    return;
                }
                self.lsps1.lock_recover().last_poll = now;
                self.follow_order(record, now).await;
            }
            None => {
                if let Some(options) = options {
                    self.make_order(options, &book, now).await;
                }
            }
        }
    }

    /// Makes an order if the room is under half the cap and the node has
    /// something to pay with.
    async fn make_order(&self, options: &LSPS1Options, book: &OrderBook, now: u64) {
        let room_msat = room_to_receive_msat(
            self.manager
                .list_usable_channels()
                .iter()
                .map(|c| c.inbound_capacity_msat),
        );
        let me = self.provider.node_id.serialize();
        if !lsps1::order_wanted(room_msat, self.cap_sat, book, now, &me) {
            return;
        }
        // Without a channel to pay from, nothing is asked: the provider is
        // not given orders that cannot be paid.
        if self.best_outbound_msat() < 1000 {
            return;
        }
        // The channels the provider has with the node now are not the order's.
        let mut prior_channels = Vec::new();
        let mut with_provider = 0;
        for c in self.manager.list_channels() {
            if c.counterparty.node_id == self.provider.node_id {
                with_provider += 1;
                if let Some(o) = c.funding_txo {
                    prior_channels.push(o.into_bitcoin_outpoint());
                }
            }
        }
        if with_provider > lsps1::MAX_PRIOR_CHANNELS {
            return;
        }
        let failed = |why: OrderWhy| {
            let record = OrderRecord {
                id: "-".to_string(),
                provider: me,
                state: OrderState::Failed,
                why,
                fee_sat: 0,
                lsp_balance_sat: 0,
                sent_msat: 0,
                payment_hash: [0; 32],
                created_unix: now,
                updated_unix: now,
                prior_channels: Vec::new(),
            };
            move |b: &mut OrderBook| {
                b.begin(record);
                b.fail(why, now);
            }
        };
        let params = match lsps1::order_params(
            options,
            room_msat,
            self.cap_sat,
            self.provider.token.as_deref(),
        ) {
            Ok(p) => p,
            Err(e) => {
                self.logger
                    .line(Level::Info, &format!("no order can be made: {e:?}"));
                self.write_failed(failed(OrderWhy::NotPossible), now).await;
                return;
            }
        };
        let provider = self.provider.node_id;
        let lsp_balance_sat = params.lsp_balance_sat;
        let asked = params.clone();
        let answer = self
            .order_request(|h| h.create_order(&provider, params, None))
            .await;
        let Some(answer) = answer else {
            self.logger
                .line(Level::Info, "the provider did not take the order");
            self.write_failed(failed(OrderWhy::Refused), now).await;
            return;
        };
        let id_ok = !answer.id.is_empty()
            && answer.id.len() <= lsps1::MAX_ID_LEN
            && answer.id.chars().all(|c| c.is_ascii_graphic());
        if !id_ok {
            self.write_failed(failed(OrderWhy::Refused), now).await;
            return;
        }
        // The order the provider approved is the one asked for, or it is not
        // paid.
        if !lsps1::terms_ok(&asked, &answer.params) {
            self.logger.line(
                Level::Info,
                "the provider's order differs from the one asked",
            );
            self.write_failed(failed(OrderWhy::NotPossible), now).await;
            return;
        }
        let record = OrderRecord {
            id: answer.id.clone(),
            provider: me,
            state: OrderState::Open,
            why: OrderWhy::None,
            fee_sat: lsps1::order_fee_sat(&answer.payment),
            lsp_balance_sat,
            sent_msat: 0,
            payment_hash: lsps1::payment_hash_of(&answer.payment),
            created_unix: now,
            updated_unix: now,
            prior_channels,
        };
        // Kept on both devices before anything is paid: a restart finds the
        // order and makes no second.
        let kept = record.clone();
        if let Err(e) = self.orders.change(|b| b.begin(kept)).await {
            self.logger
                .line(Level::Warn, &format!("order not stored: {e}"));
            return;
        }
        self.logger.line(
            Level::Info,
            &format!("order {} made, fee {} sat", record.id, record.fee_sat),
        );
        self.progress_order(&record, &provider, &answer, now).await;
    }

    /// Writes the end of an order that was never made; when the write fails
    /// the next try is still held back in memory, and the failure is logged.
    async fn write_failed(&self, f: impl FnOnce(&mut OrderBook), now: u64) {
        if let Err(e) = self.orders.change_held(f, now).await {
            self.logger.line(
                Level::Warn,
                &format!("order failure not stored, next try held back in memory: {e}"),
            );
        }
    }

    /// Asks the order's own provider about the open order and acts on the
    /// answer (SL-28): never another provider than the one it was made with.
    async fn follow_order(&self, record: &OrderRecord, now: u64) {
        let reachable = PublicKey::from_slice(&record.provider)
            .is_ok_and(|p| self.peers.peer_by_node_id(&p).is_some());
        let plan = lsps1::follow_plan(record, &self.provider.node_id.serialize(), reachable, now);
        let why = match plan {
            lsps1::FollowPlan::Ask => None,
            lsps1::FollowPlan::Wait => return,
            lsps1::FollowPlan::Fail(why) => Some(why),
        };
        // A provider id that is no key is not asked either.
        let provider = match (why, PublicKey::from_slice(&record.provider)) {
            (None, Ok(p)) => p,
            (why, _) => {
                let why = why.unwrap_or(OrderWhy::NotPossible);
                self.logger
                    .line(Level::Info, &format!("order {} over: {why:?}", record.id));
                self.write_failed(
                    |b| {
                        b.if_current(record, |b| b.fail(why, now));
                    },
                    now,
                )
                .await;
                return;
            }
        };
        let id = LSPS1OrderId(record.id.clone());
        let answer = self
            .order_request(|h| h.check_order_status(&provider, id))
            .await;
        match answer {
            Some(a) if a.id == record.id => self.progress_order(record, &provider, &a, now).await,
            _ => {
                // No answer: asked again later. A paid order that stays
                // unanswered is given up after two days, one not paid an
                // hour after it was made. The book changes only if the order
                // is still the one the decision was made from.
                if let Some(why) = lsps1::unanswered_step(record, self.sent_of(record), now) {
                    let _ = self
                        .orders
                        .change(|b| {
                            b.if_current(record, |b| b.fail(why, now));
                        })
                        .await;
                }
            }
        }
    }

    /// What the payment manager says of the payment for the order's invoice.
    /// What left is what the node set out to pay (the record's `sent_msat`);
    /// a payment claimed with none set out is waited for: the library's event
    /// brings the amount that left.
    fn sent_of(&self, record: &OrderRecord) -> lsps1::Sent {
        let id = PaymentId(record.payment_hash);
        for p in self.manager.list_recent_payments() {
            match p {
                RecentPaymentDetails::Pending { payment_id, .. }
                | RecentPaymentDetails::AwaitingInvoice { payment_id }
                    if payment_id == id =>
                {
                    return lsps1::Sent::OnItsWay;
                }
                RecentPaymentDetails::Fulfilled { payment_id, .. } if payment_id == id => {
                    return match record.sent_msat {
                        0 => lsps1::Sent::OnItsWay,
                        m => lsps1::Sent::Left(Some(m)),
                    };
                }
                _ => {}
            }
        }
        lsps1::Sent::NotSent
    }

    /// The channels the node holds, as far as an order's channel is checked.
    fn own_channels(&self) -> Vec<lsps1::OwnChannel> {
        self.manager
            .list_channels()
            .iter()
            .map(|c| lsps1::OwnChannel {
                counterparty: c.counterparty.node_id,
                funding: c.funding_txo.map(|o| o.into_bitcoin_outpoint()),
                inbound_msat: c.inbound_capacity_msat,
                ready: c.is_channel_ready,
                confirmations: c.confirmations.unwrap_or(0),
            })
            .collect()
    }

    /// What to do about an order after the provider's answer. Every change of
    /// the book applies only if the order is still the one `record` was taken
    /// from.
    /// `provider` is the order's own provider: the channel is judged against
    /// it, and the order is paid only if it is the provider chosen now.
    async fn progress_order(
        &self,
        record: &OrderRecord,
        provider: &PublicKey,
        answer: &OrderAnswer,
        now: u64,
    ) {
        let seen = lsps1::read_status(&answer.payment, &answer.channel);
        let write = |r: Result<(), lightning::io::Error>| {
            if let Err(e) = r {
                self.logger
                    .line(Level::Warn, &format!("order change not stored: {e}"));
            }
        };
        let sent = self.sent_of(record);
        let channel_ok = answer.channel.as_ref().is_some_and(|c| {
            lsps1::channel_verified(
                &self.own_channels(),
                provider,
                &c.funding_outpoint,
                record.lsp_balance_sat,
                &record.prior_channels,
            )
        });
        match lsps1::next_step(record, seen, sent, channel_ok, now) {
            lsps1::Next::Wait => {}
            lsps1::Next::MarkPaid(left_msat) => write(
                self.orders
                    .change(|b| {
                        b.if_current(record, |b| {
                            b.paid(&record.payment_hash, left_msat, now);
                        });
                    })
                    .await,
            ),
            lsps1::Next::Done => {
                self.logger
                    .line(Level::Info, &format!("order {} done", record.id));
                write(
                    self.orders
                        .change(|b| {
                            b.if_current(record, |b| {
                                b.done(now, None);
                            });
                        })
                        .await,
                );
            }
            lsps1::Next::Fail(why) => {
                self.logger
                    .line(Level::Info, &format!("order {} over: {why:?}", record.id));
                write(
                    self.orders
                        .change(|b| {
                            b.if_current(record, |b| b.fail(why, now));
                        })
                        .await,
                );
            }
            lsps1::Next::Pay => {
                // Never paid to a provider other than the order's own.
                if *provider != self.provider.node_id {
                    return;
                }
                let plan = lsps1::judge_payment(
                    &answer.payment,
                    provider,
                    self.max_opening_fee_msat,
                    self.best_outbound_msat(),
                    now,
                );
                let plan = match plan {
                    Ok(p) if p.payment_hash == record.payment_hash => p,
                    // The provider changed the invoice after the order was made.
                    Ok(_) => {
                        write(
                            self.orders
                                .change(|b| {
                                    b.if_current(record, |b| b.fail(OrderWhy::NotPossible, now));
                                })
                                .await,
                        );
                        return;
                    }
                    Err(r) => {
                        self.logger
                            .line(Level::Info, &format!("order {} not paid: {r:?}", record.id));
                        write(
                            self.orders
                                .change(|b| {
                                    b.if_current(record, |b| b.fail(r.why(), now));
                                })
                                .await,
                        );
                        return;
                    }
                };
                // The route first, so that nothing is set out with no way to
                // send it; then the book, which keeps what is set out before
                // the money goes. It is sent only if that write changed the
                // book: an order already set out, or no longer the one the
                // decision was made from, sends nothing.
                let Some(route) = self.pay_route(&plan) else {
                    return;
                };
                let set_out = self
                    .orders
                    .change_checked(|b| {
                        b.if_current(record, |b| {
                            if let Some(o) = b.order.as_mut().filter(|o| o.sent_msat == 0) {
                                o.sent_msat = plan.amount_msat;
                            }
                        });
                    })
                    .await;
                match set_out {
                    Ok(true) => {
                        // Nothing went out: what was set out is taken back
                        // and the order fails as not paid, so that it does
                        // not stay open with a payment that does not exist.
                        if !self.send_order_payment(route, &plan) {
                            write(
                                self.orders
                                    .change(|b| {
                                        b.if_current(record, |b| {
                                            b.not_sent(now);
                                        });
                                    })
                                    .await,
                            );
                        }
                    }
                    Ok(false) => {}
                    Err(e) => self
                        .logger
                        .line(Level::Warn, &format!("order change not stored: {e}")),
                }
            }
        }
    }

    /// The route to pay the provider's invoice: one hop over a usable channel
    /// to the provider itself, the one way this node pays (SL-28). The payment
    /// id is the payment hash, so the library refuses a second payment of the
    /// same invoice. `None` with no such channel.
    fn pay_route(&self, plan: &lsps1::PayPlan) -> Option<Route> {
        let channel = self
            .manager
            .list_usable_channels()
            .into_iter()
            .filter(|c| {
                c.counterparty.node_id == self.provider.node_id
                    && c.next_outbound_htlc_limit_msat >= plan.amount_msat
            })
            .max_by_key(|c| c.next_outbound_htlc_limit_msat);
        let scid = channel.and_then(|c| c.get_outbound_payment_scid())?;
        Some(Route {
            paths: vec![Path {
                hops: vec![RouteHop {
                    pubkey: self.provider.node_id,
                    node_features: NodeFeatures::empty(),
                    short_channel_id: scid,
                    channel_features: ChannelFeatures::empty(),
                    fee_msat: plan.amount_msat,
                    cltv_expiry_delta: plan.min_final_cltv,
                    maybe_announced_channel: false,
                }],
                blinded_tail: None,
            }],
            route_params: None,
        })
    }

    /// Sends the order's payment over the route. `false` if the library says
    /// it was not sent (a duplicate is on its way already, so it is not that).
    fn send_order_payment(&self, route: Route, plan: &lsps1::PayPlan) -> bool {
        match self.manager.send_payment_with_route(
            route,
            PaymentHash(plan.payment_hash),
            RecipientOnionFields::secret_only(PaymentSecret(plan.payment_secret)),
            PaymentId(plan.payment_hash),
        ) {
            Ok(()) => {
                self.logger.line(
                    Level::Info,
                    &format!("paying the provider {} msat for room", plan.amount_msat),
                );
                true
            }
            // Already on its way: nothing to do.
            Err(RetryableSendFailure::DuplicatePayment) => true,
            Err(e) => {
                self.logger.line(
                    Level::Warn,
                    &format!("the order's payment was not sent: {e:?}"),
                );
                false
            }
        }
    }

    /// Buys the offer for this amount: the provider's answer is the short
    /// channel id and the CLTV delta for the invoice's route hint.
    async fn buy(&self, amount_msat: u64, offer: &Offer) -> Result<(u64, u32), InvoiceError> {
        let handler = self
            .liquidity
            .lsps2_client_handler()
            .ok_or_else(|| InvoiceError::Provider("no handler".into()))?;
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut waiters = self.waiters.lock_recover();
            let id = handler
                .select_opening_params(
                    self.provider.node_id,
                    Some(amount_msat),
                    offer.params.clone(),
                )
                .map_err(|e| InvoiceError::Provider(format!("{e:?}")))?;
            waiters.insert(id.clone(), Waiter::Buy(tx));
            id
        };
        answer_within(&self.waiters, &id, rx, PROVIDER_ANSWER)
            .await
            .ok_or_else(|| InvoiceError::Provider("no answer or refused".into()))
    }

    /// The invoice for an amount that opens a channel: the provider's
    /// intercept channel as the one route hint (bLIP-52).
    fn jit_invoice(
        &self,
        amount_msat: u64,
        description: Description,
        scid: u64,
        cltv_delta: u32,
    ) -> Result<Bolt11Invoice, InvoiceError> {
        let lib = |e: &dyn std::fmt::Debug| InvoiceError::Library(format!("{e:?}"));
        let (payment_hash, payment_secret) = self
            .manager
            .create_inbound_payment(
                Some(amount_msat),
                INVOICE_EXPIRY_SECS,
                Some(JIT_MIN_FINAL_CLTV),
            )
            .map_err(|_| lib(&"inbound payment"))?;
        let hint = RouteHint(vec![RouteHintHop {
            src_node_id: self.provider.node_id,
            short_channel_id: scid,
            fees: lightning::routing::gossip::RoutingFees {
                base_msat: 0,
                proportional_millionths: 0,
            },
            cltv_expiry_delta: cltv_delta as u16,
            htlc_minimum_msat: None,
            htlc_maximum_msat: None,
        }]);
        let currency: Currency = self.network.into();
        let secret_key = self.keys.get_node_secret_key();
        let invoice = InvoiceBuilder::new(currency)
            .description(description.into_inner().to_string())
            .payment_hash(sha256::Hash::from_slice(&payment_hash.0).map_err(|e| lib(&e))?)
            .payment_secret(payment_secret)
            .current_timestamp()
            .min_final_cltv_expiry_delta(JIT_MIN_FINAL_CLTV as u64)
            .expiry_time(Duration::from_secs(INVOICE_EXPIRY_SECS as u64))
            .private_route(hint)
            .amount_milli_satoshis(amount_msat)
            .basic_mpp()
            .build_signed(|hash| Secp256k1::new().sign_ecdsa_recoverable(hash, &secret_key))
            .map_err(|e| lib(&e))?;
        Ok(invoice)
    }

    /// Stores what is expected of an invoice on both devices before the
    /// invoice is handed back (SL-20, SL-24, SL-29).
    async fn keep_expectation(
        &self,
        payment_hash: [u8; 32],
        max_fee_msat: u64,
    ) -> Result<(), InvoiceError> {
        let now = now_secs();
        let expectation = Expectation {
            max_fee_msat,
            expires_unix: now + INVOICE_EXPIRY_SECS as u64,
        };
        self.expectations
            .record(payment_hash, expectation, now)
            .await
            .map_err(|e| {
                self.logger
                    .line(Level::Warn, &format!("invoice not stored: {e}"));
                InvoiceError::NotStored
            })
    }
}

/// Hands each LSPS1 answer to the call that waits for it (SL-28).
fn lsps1_event(shared: &Shared, event: LSPS1ClientEvent) {
    let provider = shared.provider.node_id;
    let mut waiters = shared.waiters.lock_recover();
    match event {
        LSPS1ClientEvent::SupportedOptionsReady {
            request_id,
            counterparty_node_id,
            supported_options,
        } if counterparty_node_id == provider => {
            if let Some(Waiter::Options(tx)) = waiters.remove(&request_id) {
                let _ = tx.send(supported_options);
            }
        }
        LSPS1ClientEvent::OrderCreated {
            request_id,
            counterparty_node_id,
            order_id,
            order,
            payment,
            channel,
        }
        | LSPS1ClientEvent::OrderStatus {
            request_id,
            counterparty_node_id,
            order_id,
            order,
            payment,
            channel,
        } if counterparty_node_id == provider => {
            if let Some(Waiter::Order(tx)) = waiters.remove(&request_id) {
                let _ = tx.send(OrderAnswer {
                    id: order_id.0,
                    params: order,
                    payment,
                    channel,
                });
            }
        }
        LSPS1ClientEvent::SupportedOptionsRequestFailed { request_id, .. }
        | LSPS1ClientEvent::OrderRequestFailed { request_id, .. } => {
            // The sender is dropped: the waiting call sees a refusal.
            waiters.remove(&request_id);
        }
        _ => {}
    }
}

/// Room bought ahead (SL-28, SL-35): when the provider connects, ask it for
/// its LSPS1 options; with them, order a channel whenever the room to receive
/// is under half the cap, and follow the order until the channel is opened.
async fn lsps1_task(shared: Arc<Shared>) {
    let id = shared.provider.node_id;
    let mut was_connected = false;
    loop {
        tokio::select! {
            _ = shared.stop.notified() => return,
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
        }
        if shared.peers.peer_by_node_id(&id).is_none() {
            was_connected = false;
            continue;
        }
        if !was_connected {
            was_connected = true;
            shared.ask_lsps1_options().await;
        }
        let options = {
            let st = shared.lsps1.lock_recover();
            match (st.offer, &st.options) {
                (lsps1::Offer::Offered, Some(o)) => Some(o.clone()),
                _ => None,
            }
        };
        // An order already made is followed whatever the offer; a new one
        // needs the options.
        shared.lsps1_step(options.as_ref()).await;
    }
}

async fn rpc_chain(rpc: &RpcClient) -> Result<String, String> {
    let info = rpc
        .call_method::<serde_json::Value>("getblockchaininfo", &[])
        .await
        .map_err(|e| e.to_string())?;
    info.get("chain")
        .and_then(|c| c.as_str())
        .map(str::to_string)
        .ok_or_else(|| "chain".to_string())
}

/// How long the proxy of Tor may take to accept a connection.
const TOR_CONNECT: Duration = Duration::from_secs(10);

/// Whether something listens at the proxy's address (SL-32).
/// SL-27: the fundings of the payments that came through channels not yet
/// confirmed, judged every few seconds (the height moves with each block).
async fn watch_fundings(shared: Arc<Shared>) {
    loop {
        tokio::select! {
            _ = shared.stop.notified() => return,
            _ = tokio::time::sleep(FUNDING_CHECK_EVERY) => {}
        }
        if let Err(e) = shared.check_fundings().await {
            shared
                .logger
                .line(Level::Warn, &format!("fundings not checked: {e}"));
        }
    }
}

async fn tor_reachable(proxy: SocketAddr) -> Result<(), String> {
    match tokio::time::timeout(TOR_CONNECT, tokio::net::TcpStream::connect(proxy)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(format!("{proxy}: {e}")),
        Err(_) => Err(format!("{proxy}: no answer")),
    }
}

/// One connection to the provider through the SOCKS5 proxy of Tor, with a
/// token of its own so that it rides a circuit of its own (SL-32). The stream
/// is the one the counter's peer layer makes; it is handed to the library as
/// it is.
async fn tor_stream(
    proxy: SocketAddr,
    target: SocketAddr,
    token: u64,
) -> Result<std::net::TcpStream, String> {
    tokio::task::spawn_blocking(move || {
        openbmt_net_io::socks::connect_isolated(
            proxy,
            &openbmt_net_io::Target::Addr(target),
            TOR_CONNECT,
            Duration::from_secs(openbmt_net_io::socks::HANDSHAKE_SECS),
            Some(token),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Keeps one connection to the provider up. Nothing listens: the node takes
/// no inbound connection (SL-31). With Tor on the connection is made through
/// the proxy only; when that fails the node waits and tries the proxy again,
/// and never connects over the clear net instead (SL-32).
async fn keep_provider_connected(shared: Arc<Shared>) {
    let addr: SocketAddr = shared.provider.addr;
    let id = shared.provider.node_id;
    loop {
        if shared.peers.peer_by_node_id(&id).is_none() {
            match shared.tor {
                None => {
                    if let Some(connection) =
                        lightning_net_tokio::connect_outbound(Arc::clone(&shared.peers), id, addr)
                            .await
                    {
                        tokio::spawn(connection);
                    }
                }
                Some(proxy) => {
                    let entropy: [u8; 32] =
                        lightning::sign::EntropySource::get_secure_random_bytes(&*shared.keys);
                    let token = u64::from_le_bytes(entropy[..8].try_into().unwrap_or([0; 8]));
                    match tor_stream(proxy, addr, token).await {
                        Ok(stream) => {
                            tokio::spawn(lightning_net_tokio::setup_outbound(
                                Arc::clone(&shared.peers),
                                id,
                                stream,
                            ));
                        }
                        Err(e) => shared
                            .logger
                            .line(Level::Warn, &format!("tor: the provider not reached: {e}")),
                    }
                }
            }
        }
        tokio::select! {
            _ = shared.stop.notified() => return,
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
        }
    }
}

/// Hands each LSPS2 answer to the call that waits for it.
async fn lsps2_events(shared: Arc<Shared>) {
    loop {
        let event = tokio::select! {
            _ = shared.stop.notified() => return,
            e = shared.liquidity.next_event_async() => e,
        };
        let event = match event {
            LiquidityEvent::LSPS2Client(event) => event,
            LiquidityEvent::LSPS1Client(event) => {
                lsps1_event(&shared, event);
                continue;
            }
            _ => continue,
        };
        let mut waiters = shared.waiters.lock_recover();
        match event {
            LSPS2ClientEvent::OpeningParametersReady {
                request_id,
                counterparty_node_id,
                opening_fee_params_menu,
            } if counterparty_node_id == shared.provider.node_id => {
                if let Some(Waiter::Params(tx)) = waiters.remove(&request_id) {
                    let _ = tx.send(opening_fee_params_menu);
                }
            }
            LSPS2ClientEvent::InvoiceParametersReady {
                request_id,
                counterparty_node_id,
                intercept_scid,
                cltv_expiry_delta,
                ..
            } if counterparty_node_id == shared.provider.node_id => {
                if let Some(Waiter::Buy(tx)) = waiters.remove(&request_id) {
                    let _ = tx.send((intercept_scid, cltv_expiry_delta));
                }
            }
            LSPS2ClientEvent::GetInfoFailed { request_id, .. }
            | LSPS2ClientEvent::BuyRequestFailed { request_id, .. } => {
                // The sender is dropped: the waiting call sees a refusal.
                waiters.remove(&request_id);
            }
            _ => {}
        }
    }
}

/// The library's events (SL-01, SL-21, SL-29, SL-31).
async fn handle_event(shared: Arc<Shared>, event: Event) -> Result<(), ReplayEvent> {
    let log = |l: Level, m: String| shared.logger.line(l, &m);
    match event {
        Event::OpenChannelRequest {
            temporary_channel_id,
            counterparty_node_id,
            funding_satoshis,
            ..
        } => {
            // Only the chosen provider opens a channel (SL-01, SL-31).
            if counterparty_node_id != shared.provider.node_id {
                log(
                    Level::Warn,
                    format!("refused a channel from {counterparty_node_id}"),
                );
                let _ = shared.manager.force_close_broadcasting_latest_txn(
                    &temporary_channel_id,
                    &counterparty_node_id,
                    "refused".to_string(),
                );
                return Ok(());
            }
            let overrides = Some(ChannelConfigOverrides {
                handshake_overrides: Some(ChannelHandshakeConfigUpdate {
                    // The provider's first payment is the whole of what the
                    // channel will carry at once.
                    max_inbound_htlc_value_in_flight_percent_of_channel: Some(100),
                    // The window to be back online (SL-30): the most the
                    // provider allows, up to 2016. A provider that allows less
                    // than 1008 was refused when its offer was chosen.
                    to_self_delay: Some(crate::provider::WANTED_TO_SELF_DELAY as u16),
                    ..Default::default()
                }),
                update_overrides: Some(ChannelConfigUpdate {
                    // The provider takes its fee out of the first payment.
                    accept_underpaying_htlcs: Some(true),
                    ..Default::default()
                }),
            });
            let result = if shared.provider.zero_conf {
                shared
                    .manager
                    .accept_inbound_channel_from_trusted_peer_0conf(
                        &temporary_channel_id,
                        &counterparty_node_id,
                        0,
                        overrides,
                    )
            } else {
                shared.manager.accept_inbound_channel(
                    &temporary_channel_id,
                    &counterparty_node_id,
                    0,
                    overrides,
                )
            };
            log(
                Level::Info,
                format!("channel of {funding_satoshis} sat from the provider: {result:?}"),
            );
        }
        Event::PaymentClaimable {
            payment_hash,
            purpose,
            amount_msat,
            counterparty_skimmed_fee_msat,
            receiving_channel_ids,
            ..
        } => {
            // Only invoices this node made, not expired, and no more skimmed
            // than agreed (SL-29), also for an invoice made before a restart.
            let claim = shared.expectations.judge(
                &payment_hash.0,
                counterparty_skimmed_fee_msat,
                now_secs(),
            );
            let preimage = match &purpose {
                PaymentPurpose::Bolt11InvoicePayment { .. } => purpose.preimage(),
                _ => None,
            };
            // Never release the secret for a just-in-time payment until every
            // channel that carried it has funding confirmed to the shop's
            // policy depth. The later PaymentClaimed funding record is for
            // cashier policy and cannot undo an already-released preimage.
            let receiving_channels: Vec<[u8; 32]> = receiving_channel_ids
                .iter()
                .map(|(channel_id, _)| channel_id.0)
                .collect();
            let channel_fundings: Vec<ClaimChannelFunding> = shared
                .manager
                .list_channels()
                .iter()
                .map(|channel| ClaimChannelFunding {
                    channel_id: channel.channel_id.0,
                    confirmations: channel.confirmations,
                    has_funding_output: channel.funding_txo.is_some()
                        && channel.get_funding_output().is_some(),
                })
                .collect();
            let funding_ready =
                claim_funding_ready(&receiving_channels, &channel_fundings, shared.confirm_depth);
            let allowed = claim == Claim::Allowed && funding_ready;
            match (preimage, allowed) {
                (Some(p), true) => shared.manager.claim_funds(p),
                _ => {
                    log(
                        Level::Warn,
                        format!("refused a payment of {amount_msat} msat, skimmed {counterparty_skimmed_fee_msat}: {claim:?}, funding_ready={funding_ready}"),
                    );
                    shared.manager.fail_htlc_backwards(&payment_hash);
                }
            }
        }
        Event::PaymentClaimed {
            payment_hash,
            amount_msat,
            sender_intended_total_msat,
            htlcs,
            ..
        } => {
            // Both copies hold the claim: the library gave this event only
            // after the monitor update was complete (SL-21).
            // Kept until the caller acknowledges it, and only then given: a
            // crash after this gives it again on the next start, and a
            // failure to keep it makes the library give the event again.
            let now = now_secs();
            // SL-40: a channel the payment came through whose funding has no
            // confirmation yet.
            let channels: Vec<ChannelFunding> = shared
                .manager
                .list_channels()
                .iter()
                .map(ChannelFunding::of)
                .collect();
            let height = shared.manager.current_best_block().height;
            shared.note_fundings(&channels, height);
            let htlc_channels: Vec<[u8; 32]> = htlcs.iter().map(|h| h.channel_id.0).collect();
            let unconfirmed = funding::unconfirmed_fundings(
                &htlc_channels,
                &channels,
                shared.confirm_depth,
                |id| {
                    shared
                        .last_fundings
                        .lock_recover()
                        .get(id)
                        .map(|&(txid, confirmations, _)| (txid, confirmations))
                },
            );
            let record = match shared
                .paid_log
                .record(PaidRecord {
                    payment_hash: payment_hash.0,
                    amount_msat,
                    skimmed_msat: htlcs
                        .iter()
                        .map(|h| h.counterparty_skimmed_fee_msat)
                        .fold(0, u64::saturating_add),
                    sender_intended_msat: sender_intended_total_msat,
                    received_unix: now,
                    unconfirmed_fundings: unconfirmed.txids,
                    more_fundings: unconfirmed.more,
                    lost_unknown: unconfirmed.lost_unknown,
                })
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    log(Level::Warn, format!("paid event not kept: {e}"));
                    return Err(ReplayEvent());
                }
            };
            if let Err(e) = shared.expectations.forget(&payment_hash.0, now).await {
                log(Level::Warn, format!("expectation not dropped: {e}"));
                return Err(ReplayEvent());
            }
            // None: the caller has acknowledged this sale already, and this is
            // the library's repeat of its event after a crash.
            if let Some(record) = record {
                for txid in &record.unconfirmed_fundings {
                    // A funding seen in a block, through a channel open now
                    // or one gone since, is watched with that block, so that
                    // a close before the next check is not a loss (SL-27).
                    let seen = funding::last_seen_height(txid, &htlc_channels, |id| {
                        shared.last_fundings.lock_recover().get(id).copied()
                    });
                    match shared.funding_log.watch_seen(*txid, height, seen).await {
                        // One settled already: the counter is told again, so
                        // that it always hears it (SL-27, SL-40).
                        Ok(Some(outcome)) => {
                            let _ = shared.funding.send(FundingEvent {
                                txid: *txid,
                                outcome,
                            });
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log(Level::Warn, format!("funding not watched: {e}"));
                            return Err(ReplayEvent());
                        }
                    }
                }
                // A funding named for a channel already gone is judged now, not
                // at the next pass: lost, unless it was seen in a block.
                if !record.unconfirmed_fundings.is_empty() {
                    if let Err(e) = shared.check_fundings().await {
                        log(Level::Warn, format!("fundings not checked: {e}"));
                    }
                }
                let _ = shared.paid.send(Paid::from(record));
            }
        }
        Event::PaymentSent {
            payment_hash,
            amount_msat,
            ..
        } => {
            // The provider's invoice for room bought ahead is paid (SL-28);
            // what left counts with the opening fees paid (SL-29, SL-45). A
            // failure to keep it gives the event again.
            let now = now_secs();
            if let Err(e) = shared
                .orders
                .change(|b| {
                    b.paid(&payment_hash.0, amount_msat, now);
                })
                .await
            {
                log(Level::Warn, format!("order payment not kept: {e}"));
                return Err(ReplayEvent());
            }
        }
        Event::PaymentFailed { payment_hash, .. } => {
            let (now, hash) = (now_secs(), payment_hash.map(|h| h.0));
            if let Some(hash) = hash {
                if let Err(e) = shared
                    .orders
                    .change(|b| {
                        b.payment_failed(&hash, now);
                    })
                    .await
                {
                    log(Level::Warn, format!("order payment failure not kept: {e}"));
                    return Err(ReplayEvent());
                }
            }
        }
        Event::ChannelPending {
            channel_id,
            counterparty_node_id,
            ..
        } => {
            log(
                Level::Info,
                format!("channel pending {channel_id} with {counterparty_node_id}"),
            );
        }
        Event::FundingTransactionReadyForSigning {
            channel_id,
            counterparty_node_id,
            unsigned_transaction,
            ..
        } => {
            // A splice-out adds no input of the shop's own: the transaction
            // goes back as it is and LDK signs the channel's input itself.
            match shared.manager.funding_transaction_signed(
                &channel_id,
                &counterparty_node_id,
                unsigned_transaction,
            ) {
                Ok(()) => log(Level::Info, format!("splice signed {channel_id}")),
                Err(e) => log(Level::Warn, format!("splice not signed: {e:?}")),
            }
        }
        Event::SplicePending { channel_id, .. } => {
            log(Level::Info, format!("splice pending {channel_id}"));
        }
        Event::SpliceFailed { channel_id, .. } => {
            log(Level::Warn, format!("splice failed {channel_id}"));
        }
        Event::ChannelReady {
            channel_id,
            counterparty_node_id,
            ..
        } => {
            log(
                Level::Info,
                format!("channel ready {channel_id} with {counterparty_node_id}"),
            );
            if shared.check_fundings().await.is_err() {
                return Err(ReplayEvent());
            }
        }
        Event::ChannelClosed {
            channel_id, reason, ..
        } => {
            log(
                Level::Warn,
                format!("channel closed {channel_id}: {reason}"),
            );
            // A funding that was never in a block is lost with its channel
            // (SL-27).
            if shared.check_fundings().await.is_err() {
                return Err(ReplayEvent());
            }
        }
        Event::SpendableOutputs {
            outputs,
            channel_id,
        } => {
            // The sweeper keeps them in the store and spends them to the
            // shop's address. A static output that pays the shop's own wallet
            // (the address a close was given) is already there and is left
            // out: it is not ours to sign. A failure to keep them gives the
            // event again.
            let (ours, left): (Vec<_>, Vec<_>) = outputs
                .into_iter()
                .partition(|d| crate::sweep::spendable_by_node(&shared.keys, d));
            let n = ours.len();
            if shared
                .sweeper
                .track_spendable_outputs(ours, channel_id, false, None)
                .await
                .is_err()
            {
                return Err(ReplayEvent());
            }
            log(
                Level::Info,
                format!(
                    "{n} spendable outputs tracked, {} left to the shop's wallet",
                    left.len()
                ),
            );
        }
        other => {
            log(
                Level::Debug,
                format!("event not used: {}", event_name(&other)),
            );
        }
    }
    Ok(())
}

fn event_name(e: &Event) -> &'static str {
    match e {
        Event::HTLCIntercepted { .. } => "HTLCIntercepted",
        Event::BumpTransaction(_) => "BumpTransaction",
        Event::HTLCHandlingFailed { .. } => "HTLCHandlingFailed",
        Event::DiscardFunding { .. } => "DiscardFunding",
        Event::FundingTxBroadcastSafe { .. } => "FundingTxBroadcastSafe",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bip85::node_seed;
    use crate::chain::CoreRpc;
    use crate::device::DirDevice;
    use crate::testutil::temp_dir;
    use lightning::routing::router::Router;
    use std::str::FromStr;

    fn claim_channel(
        id: u8,
        confirmations: Option<u32>,
        has_funding_output: bool,
    ) -> ClaimChannelFunding {
        ClaimChannelFunding {
            channel_id: [id; 32],
            confirmations,
            has_funding_output,
        }
    }

    #[test]
    fn claim_gate_rejects_empty_or_unknown_receiving_channel_ids() {
        assert!(!claim_funding_ready(
            &[],
            &[claim_channel(1, Some(2), true)],
            1
        ));
        assert!(!claim_funding_ready(
            &[[2; 32]],
            &[claim_channel(1, Some(2), true)],
            1
        ));
    }

    #[test]
    fn claim_gate_requires_a_known_funding_output_and_configured_depth() {
        assert!(!claim_funding_ready(
            &[[1; 32]],
            &[claim_channel(1, Some(2), false)],
            1
        ));
        assert!(!claim_funding_ready(
            &[[1; 32]],
            &[claim_channel(1, Some(1), true)],
            2
        ));
        assert!(!claim_funding_ready(
            &[[1; 32]],
            &[claim_channel(1, None, true)],
            1
        ));
    }

    #[test]
    fn claim_gate_requires_every_mpp_channel_and_accepts_confirmed_channels() {
        let ids = [[1; 32], [2; 32]];
        let channels = [
            claim_channel(1, Some(2), true),
            claim_channel(2, Some(1), true),
        ];
        assert!(!claim_funding_ready(&ids, &channels, 2));
        assert!(claim_funding_ready(&ids, &channels, 1));
    }

    /// A config whose files go in a directory of this test's own, since
    /// tests run at once and one test's start must not show in another's.
    fn config(network: Network, test: &str) -> NodeConfig {
        let dir = temp_dir(test);
        NodeConfig {
            network,
            seed: NodeSeed([3; 64]),
            data_dir: dir.join("node"),
            // Nothing listens here: a start that gets as far as Core fails.
            chain: ChainSource::Core(CoreRpc {
                host: "127.0.0.1".into(),
                port: 1,
                user: "u".into(),
                password: "p".into(),
            }),
            tor: None,
            device: Arc::new(DirDevice::new(dir.join("device")).unwrap()),
            provider: ProviderConfig {
                node_id: PublicKey::from_str(
                    "039fe56192d9a44a06a725f975e5d7ccf307e023e2eab9a6bb5114ee8f69b6a9e9",
                )
                .unwrap(),
                addr: "127.0.0.1:9".parse().unwrap(),
                token: None,
                zero_conf: false,
            },
            max_opening_fee_msat: 1_000_000,
            cap_sat: 1_000_000,
            start_from: StartFrom::Newer,
            wallet: crate::testutil::wallet_text(),
            sweep_index: 0,
            confirm_depth: 1,
        }
    }

    #[test]
    fn sl_60_mainnet_is_refused_until_the_conditions_are_met() {
        const { assert!(!MAINNET_ALLOWED) };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let c = config(Network::Bitcoin, "sl-60-mainnet");
        let data_dir = c.data_dir.clone();
        let err = rt.block_on(Node::start(c)).err().unwrap();
        assert!(matches!(err, StartError::MainnetNotYet));
        assert_eq!(err.to_string(), crate::words::SL_60);
        // Refused before anything was made on disk.
        assert!(!data_dir.exists());
    }

    /// A SOCKS5 proxy that takes one connection, signs it in with any name,
    /// and answers a connect request for an IPv4 address with success. What
    /// it was asked to connect to is sent on `asked`.
    fn fake_proxy(
        asked: std::sync::mpsc::Sender<SocketAddr>,
    ) -> (SocketAddr, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let mut greeting = [0u8; 4];
            c.read_exact(&mut greeting).unwrap();
            // The name-and-password method is offered; it is taken.
            assert_eq!(greeting, [5, 2, 0, 2]);
            c.write_all(&[5, 2]).unwrap();
            let mut head = [0u8; 2];
            c.read_exact(&mut head).unwrap();
            let mut rest = vec![0u8; 16 + 1 + 16];
            c.read_exact(&mut rest).unwrap();
            c.write_all(&[1, 0]).unwrap();
            let mut request = [0u8; 10];
            c.read_exact(&mut request).unwrap();
            assert_eq!(request[..4], [5, 1, 0, 1]);
            let ip = std::net::Ipv4Addr::new(request[4], request[5], request[6], request[7]);
            let port = u16::from_be_bytes([request[8], request[9]]);
            asked.send(SocketAddr::from((ip, port))).unwrap();
            c.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
        });
        (addr, thread)
    }

    #[test]
    fn sl_32_with_tor_on_the_provider_is_reached_through_the_proxy_with_the_stream_it_makes() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (asked_tx, asked_rx) = std::sync::mpsc::channel();
        let (proxy, thread) = fake_proxy(asked_tx);
        let provider: SocketAddr = "127.0.0.1:9735".parse().unwrap();
        let stream = rt.block_on(tor_stream(proxy, provider, 7)).unwrap();
        thread.join().unwrap();
        // The proxy was asked for the provider, and the stream is the proxy's.
        assert_eq!(asked_rx.recv().unwrap(), provider);
        assert_eq!(stream.peer_addr().unwrap(), proxy);
    }

    #[test]
    fn sl_32_a_proxy_that_refuses_gives_an_error_and_no_stream() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = listener.local_addr().unwrap();
        // The proxy takes the connection and closes it.
        let thread = std::thread::spawn(move || drop(listener.accept().unwrap()));
        let provider: SocketAddr = "127.0.0.1:9735".parse().unwrap();
        assert!(rt.block_on(tor_stream(proxy, provider, 7)).is_err());
        thread.join().unwrap();
    }

    #[test]
    fn sl_32_with_tor_on_and_no_proxy_the_node_stops_before_anything_is_made() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // Nothing listens at the proxy's address: a port that was just free.
        let free = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let mut c = config(Network::Regtest, "sl-32-no-proxy");
        c.tor = Some(free);
        let data_dir = c.data_dir.clone();
        let err = rt.block_on(Node::start(c)).err().unwrap();
        assert!(matches!(err, StartError::TorUnreachable(_)), "{err:?}");
        assert!(err
            .to_string()
            .starts_with(crate::words::SL_32_TOR_UNREACHABLE));
        // Neither Core nor the provider was asked, and nothing is on disk.
        assert!(!data_dir.exists());
    }

    #[test]
    fn sl_60_the_test_networks_get_past_the_refusal() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for n in [Network::Regtest, Network::Signet] {
            let err = rt
                .block_on(Node::start(config(n, "sl-60-test-networks")))
                .err()
                .unwrap();
            assert!(matches!(err, StartError::Chain(_)), "{err}");
        }
    }

    #[test]
    fn sl_01_channels_are_unannounced_accepted_by_hand_and_nothing_is_forwarded() {
        let c = user_config();
        assert!(!c.channel_handshake_config.announce_for_forwarding);
        assert!(
            c.channel_handshake_limits
                .force_announced_channel_preference
        );
        assert!(c.manually_accept_inbound_channels);
        assert!(!c.accept_forwards_to_priv_channels);
        assert!(!c.accept_intercept_htlcs);
    }

    #[test]
    fn sl_29_every_channel_takes_underpaying_htlcs_and_the_claim_checks_stay() {
        // The provider's fee is taken from a payment on any channel with it.
        assert!(user_config().channel_config.accept_underpaying_htlcs);
        // What is claimed is still judged by the recorded invoice.
        assert!(!user_config().accept_intercept_htlcs);
        assert!(user_config().manually_accept_inbound_channels);
    }

    #[test]
    fn sl_33_a_wallet_that_does_not_read_is_refused_before_anything_is_made() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for bad in [
            "",
            "not a descriptor",
            // A test-network key is refused as the counter's import refuses it.
            "wpkh([00000000/84h/1h/0h]tpubD6NzVbkrYhZ4XgiXtGrdW5XDAPFCL9h7we1vwNCpn8tGbBcgfVYjXyhWo4E1xkh56hjod1RhGjxbaTLV3X4FyWuejifB9jusQ46QzG87VKp/<0;1>/*)",
        ] {
            let mut c = config(Network::Regtest, "sl-33-wallet");
            c.wallet = bad.to_string();
            let data_dir = c.data_dir.clone();
            let err = rt.block_on(Node::start(c)).err().unwrap();
            assert!(!data_dir.exists(), "{bad}");
            assert!(matches!(err, StartError::WalletRefused), "{err}");
            assert_eq!(err.to_string(), crate::words::SL_33_DESTINATION_REFUSED);
        }
        // An index with no address in the wallet is refused the same way.
        let mut c = config(Network::Regtest, "sl-33-index");
        c.sweep_index = 1 << 31;
        let data_dir = c.data_dir.clone();
        let err = rt.block_on(Node::start(c)).err().unwrap();
        assert!(matches!(err, StartError::WalletRefused), "{err}");
        assert!(!data_dir.exists());
    }

    #[test]
    fn sl_30_the_node_asks_for_2016_blocks() {
        assert_eq!(
            user_config().channel_handshake_config.our_to_self_delay,
            2016
        );
    }

    #[test]
    fn sl_01_the_router_finds_no_route() {
        let params = lightning::routing::router::RouteParameters::from_payment_params_and_value(
            lightning::routing::router::PaymentParameters::from_node_id(
                PublicKey::from_str(
                    "039fe56192d9a44a06a725f975e5d7ccf307e023e2eab9a6bb5114ee8f69b6a9e9",
                )
                .unwrap(),
                40,
            ),
            1000,
        );
        let me = PublicKey::from_str(
            "039fe56192d9a44a06a725f975e5d7ccf307e023e2eab9a6bb5114ee8f69b6a9e9",
        )
        .unwrap();
        assert!(NoRouter
            .find_route(
                &me,
                &params,
                None,
                lightning::routing::router::InFlightHtlcs::new()
            )
            .is_err());
    }

    #[test]
    fn sl_10_the_seed_of_the_specs_words_gives_the_node_key_of_the_prototype() {
        // The BIP-85 text's master key; its seed gives the node id the
        // prototype of 2026-11-12 had on two fresh rebuilds.
        let master = Xpriv::from_str("xprv9s21ZrQH143K2LBWUUQRFXhucrQqBpKdRRxNVq2zBqsx8HVqFk2uYo8kmbaLLHRdqtQpUm98uKfu3vca1LqdGhUtyoFnCNkfmXRyPXLjbKb").unwrap();
        let seed = node_seed(&master, 0).unwrap();
        let node_master = Xpriv::new_master(Network::Regtest, &seed.0).unwrap();
        let keys = KeysManager::new(&node_master.private_key.secret_bytes(), 1, 1, true);
        let id = keys.get_node_id(lightning::sign::Recipient::Node).unwrap();
        assert_eq!(
            id.to_string(),
            "039fe56192d9a44a06a725f975e5d7ccf307e023e2eab9a6bb5114ee8f69b6a9e9"
        );
        // And the same again from a second start, at another time.
        let again = KeysManager::new(&node_master.private_key.secret_bytes(), 99, 7, true);
        assert_eq!(
            again.get_node_id(lightning::sign::Recipient::Node).unwrap(),
            id
        );
    }

    #[test]
    fn sl_10_the_second_index_is_another_node() {
        let master = Xpriv::from_str("xprv9s21ZrQH143K2LBWUUQRFXhucrQqBpKdRRxNVq2zBqsx8HVqFk2uYo8kmbaLLHRdqtQpUm98uKfu3vca1LqdGhUtyoFnCNkfmXRyPXLjbKb").unwrap();
        let seed = node_seed(&master, 1).unwrap();
        let node_master = Xpriv::new_master(Network::Regtest, &seed.0).unwrap();
        let keys = KeysManager::new(&node_master.private_key.secret_bytes(), 1, 1, true);
        assert_eq!(
            keys.get_node_id(lightning::sign::Recipient::Node)
                .unwrap()
                .to_string(),
            "02a5f163beb545f174bd305e1f3509ff04051164a4cf6271cdec273c6c0d401116"
        );
    }

    #[test]
    fn sl_72_the_cores_id_is_the_librarys_for_the_vector_and_two_other_seeds() {
        let master = Xpriv::from_str("xprv9s21ZrQH143K2LBWUUQRFXhucrQqBpKdRRxNVq2zBqsx8HVqFk2uYo8kmbaLLHRdqtQpUm98uKfu3vca1LqdGhUtyoFnCNkfmXRyPXLjbKb").unwrap();
        let vector = node_seed(&master, 0).unwrap();
        let second = node_seed(&master, 1).unwrap();
        let counted: [u8; 64] = std::array::from_fn(|i| (i as u8).wrapping_mul(7).wrapping_add(3));
        let mut ids = Vec::new();
        for seed in [vector.0, second.0, counted] {
            // The node's own path to the library's key (SL-10).
            let node_master = Xpriv::new_master(Network::Regtest, &seed).unwrap();
            let keys = KeysManager::new(&node_master.private_key.secret_bytes(), 1, 1, true);
            let library = keys.get_node_id(lightning::sign::Recipient::Node).unwrap();
            let core = openbmt_core::shop_lightning::node_id(&seed);
            assert_eq!(core, library.serialize());
            ids.push(core);
        }
        // The vector's ID is the prototype's, and the three differ.
        assert_eq!(
            ids[0]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "039fe56192d9a44a06a725f975e5d7ccf307e023e2eab9a6bb5114ee8f69b6a9e9"
        );
        assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2]);
    }

    #[tokio::test]
    async fn sl_28_a_waiter_whose_answer_does_not_come_is_taken_out_of_the_map() {
        let waiters = Mutex::new(HashMap::new());
        let id = LSPSRequestId("r1".to_string());
        let (tx, rx) = oneshot::channel();
        waiters.lock().unwrap().insert(id.clone(), Waiter::Buy(tx));
        let got = answer_within(&waiters, &id, rx, Duration::from_millis(20)).await;
        assert_eq!(got, None);
        assert!(waiters.lock().unwrap().is_empty());
        // One that is answered gives the answer; the event takes its entry.
        let id = LSPSRequestId("r2".to_string());
        let (tx, rx) = oneshot::channel();
        waiters.lock().unwrap().insert(id.clone(), Waiter::Buy(tx));
        if let Some(Waiter::Buy(tx)) = waiters.lock().unwrap().remove(&id) {
            tx.send((7, 8)).unwrap();
        }
        let got = answer_within(&waiters, &id, rx, Duration::from_secs(5)).await;
        assert_eq!(got, Some((7, 8)));
        assert!(waiters.lock().unwrap().is_empty());
    }
}
