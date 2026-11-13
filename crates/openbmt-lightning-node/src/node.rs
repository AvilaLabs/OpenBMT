//! The node: the library wired as the rules say.
//!
//! - Receive only (SL-01): no public way to pay or forward, and channels are
//!   opened by the provider alone.
//! - The seed is the 64 bytes of SL-10; the library never takes the words.
//! - State on two devices (SL-20 to SL-25), through `store.rs`.
//! - Chain data from the shop's own Bitcoin Core (SL-26), `chain.rs`.
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
use crate::chain::{ChainFilter, CoreBroadcaster, CoreFees, CoreRpc};
use crate::device::{DeviceSnapshot, DeviceState, DeviceStatus, SecondDevice};
use crate::expect::{Claim, Expectation, Expectations};
use crate::logger::Logger;
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
    Bolt11InvoiceParameters, ChainParameters, ChannelManagerReadArgs,
};
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
use lightning::onion_message::messenger::NullMessageRouter;
use lightning::routing::router::{RouteHint, RouteHintHop};
use lightning::sign::{InMemorySigner, KeysManager, NodeSigner};
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
use lightning_liquidity::lsps2::client::LSPS2ClientConfig;
use lightning_liquidity::lsps2::event::LSPS2ClientEvent;
use lightning_liquidity::lsps2::msgs::LSPS2OpeningFeeParams;
use lightning_liquidity::LiquidityClientConfig;
use openbmt_core::pinned_wallet::PinnedWallet;
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
    /// The shop's Bitcoin Core.
    pub core: CoreRpc,
    /// The second device (SL-20).
    pub device: Arc<dyn SecondDevice>,
    /// The provider (SL-28).
    pub provider: ProviderConfig,
    /// The most the shop pays for one opening, in millisatoshis (SL-29).
    pub max_opening_fee_msat: u64,
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
    /// What the payer sent, if the library knows it.
    pub sender_intended_msat: Option<u64>,
    /// When the claim was seen, in seconds since the epoch. A repeat of the
    /// event carries the time of the first.
    pub received_unix: u64,
}

impl From<PaidRecord> for Paid {
    fn from(r: PaidRecord) -> Self {
        Paid {
            payment_hash: r.payment_hash,
            amount_msat: r.amount_msat,
            sender_intended_msat: r.sender_intended_msat,
            received_unix: r.received_unix,
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
}

enum Waiter {
    Params(oneshot::Sender<Vec<LSPS2OpeningFeeParams>>),
    Buy(oneshot::Sender<(u64, u32)>),
}

struct Shared {
    network: Network,
    provider: ProviderConfig,
    max_opening_fee_msat: u64,
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
    waiters: Mutex<HashMap<LSPSRequestId, Waiter>>,
    expectations: Expectations,
    paid_log: PaidLog,
    sweeper: Arc<Sweeper>,
    destination: Arc<SweepDestination>,
    chain_height: AtomicU64,
    last_poll_ok: AtomicU64,
    stopping: AtomicBool,
    stop: Notify,
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
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
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
        let seal_key = SealKey::from_seed(&config.seed);

        // Chain data (SL-26).
        let rpc = Arc::new(config.core.client());
        let chain_name = rpc_chain(&rpc).await.map_err(StartError::Chain)?;
        if !network_name_ok(config.network, &chain_name) {
            return Err(StartError::Chain(format!("core is on {chain_name}")));
        }
        let fees = Arc::new(CoreFees::default());
        fees.refresh(&rpc).await;
        let broadcaster = Arc::new(CoreBroadcaster::new(
            Arc::clone(&rpc),
            handle.clone(),
            Arc::clone(&logger),
        ));
        let filter = Arc::new(ChainFilter::default());

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
        let now_unix = now_secs();
        let expectations = Expectations::load(Arc::clone(&store), now_unix)
            .await
            .map_err(setup)?;
        let paid_log = PaidLog::new(Arc::clone(&background));
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
        let (manager, chain_tip, monitors) = match manager_bytes {
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
                let tip = synchronize_listeners(Arc::clone(&rpc), config.network, &mut cache, set)
                    .await
                    .map_err(|e| StartError::Chain(format!("{e:?}")))?;
                (
                    manager,
                    tip,
                    listeners.into_iter().map(|(_, l)| l.0).collect::<Vec<_>>(),
                )
            }
            None => {
                let tip = match &kept_sweeper {
                    // Spendable outputs are tracked: the sweeper is brought to
                    // the tip, the manager is new there.
                    Some((best, sweeper)) => synchronize_listeners(
                        Arc::clone(&rpc),
                        config.network,
                        &mut cache,
                        vec![(best.block_hash, &**sweeper as &dyn Listen)],
                    )
                    .await
                    .map_err(|e| StartError::Chain(format!("{e:?}")))?,
                    None => validate_best_block_header(Arc::clone(&rpc))
                        .await
                        .map_err(|e| StartError::Chain(format!("{e:?}")))?,
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
                        best_block: tip.to_best_block(),
                    },
                    now.as_secs() as u32,
                );
                (manager, tip, monitors.into_iter().map(|(_, m)| m).collect())
            }
        };
        let manager = Arc::new(manager);
        let sweeper = match kept_sweeper {
            Some((_, sweeper)) => sweeper,
            None => sweeper_parts.new_at(chain_tip.to_best_block()),
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
                    best_block: chain_tip.to_best_block(),
                }),
                Arc::clone(&background),
                Arc::clone(&broadcaster),
                None,
                Some(LiquidityClientConfig {
                    lsps1_client_config: None,
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
        // The paid events the caller has not acknowledged come again, with the
        // same payment hash.
        for record in paid_log.pending(now_unix).await.map_err(setup)? {
            let _ = paid_tx.send(Paid::from(record));
        }
        let shared = Arc::new(Shared {
            network: config.network,
            provider: config.provider.clone(),
            max_opening_fee_msat: config.max_opening_fee_msat,
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
            waiters: Mutex::new(HashMap::new()),
            expectations,
            paid_log,
            sweeper: Arc::clone(&sweeper),
            destination: Arc::clone(&destination),
            chain_height: AtomicU64::new(chain_tip.height as u64),
            last_poll_ok: AtomicU64::new(now_secs()),
            stopping: AtomicBool::new(false),
            stop: Notify::new(),
        });

        // Channels the provider opened before the setting was made take
        // underpaying HTLCs too (SL-29, SL-31).
        accept_underpaying_on_open_channels(&manager, &config.provider.node_id, &logger);

        let mut tasks = vec![flusher];

        // Chain: poll Core for the tip and feed the monitor and the manager.
        {
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

        // The provider: stay connected to it, and only to it (SL-31).
        {
            let shared = Arc::clone(&shared);
            tasks.push(tokio::spawn(async move {
                keep_provider_connected(shared).await
            }));
        }
        // The LSPS2 answers.
        {
            let shared = Arc::clone(&shared);
            tasks.push(tokio::spawn(async move { lsps2_events(shared).await }));
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

        logger.line(
            Level::Info,
            &format!(
                "started, node id {}",
                keys.get_node_id(lightning::sign::Recipient::Node)
                    .map(|k| k.to_string())
                    .unwrap_or_default()
            ),
        );
        Ok(Node {
            shared,
            paid_rx: tokio::sync::Mutex::new(paid_rx),
            tasks: Mutex::new(tasks),
            processor: Mutex::new(Some(processor)),
        })
    }

    /// The node's public key.
    pub fn node_id(&self) -> PublicKey {
        self.shared
            .keys
            .get_node_id(lightning::sign::Recipient::Node)
            .expect("the node's own key")
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

    /// What the manager reads.
    pub fn status(&self) -> NodeStatus {
        let s = &self.shared;
        let channels = s.manager.list_channels();
        let usable = s.manager.list_usable_channels();
        NodeStatus {
            device: s.device_status.snapshot(),
            chain_ok: s.chain_ok(),
            chain_height: s.chain_height.load(Ordering::SeqCst) as u32,
            provider_connected: s.peers.peer_by_node_id(&s.provider.node_id).is_some(),
            channels: channels.len(),
            usable_channels: usable.len(),
            room_msat: room_to_receive_msat(usable.iter().map(|c| c.inbound_capacity_msat)),
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
    /// address at `index`, which the node derives itself (SL-36).
    pub fn close_all(&self, index: u32) -> Result<usize, String> {
        let to = self.wallet_address(index)?;
        crate::close::close_all(&self.shared.manager, &to)
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
        let tasks: Vec<_> = std::mem::take(&mut *self.tasks.lock().unwrap());
        let processor = self.processor.lock().unwrap().take();
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
        // What the background owes the second device, once more.
        let _ =
            tokio::time::timeout(Duration::from_secs(5), self.shared.store.flush_lazy_once()).await;
        self.shared.peers.disconnect_all_peers();
    }
}

impl Shared {
    fn chain_ok(&self) -> bool {
        now_secs().saturating_sub(self.last_poll_ok.load(Ordering::SeqCst)) < CHAIN_STALE_SECS
    }

    /// Asks the provider for its offers (SL-28).
    async fn opening_params(&self) -> Result<Vec<LSPS2OpeningFeeParams>, ()> {
        let handler = self.liquidity.lsps2_client_handler().ok_or(())?;
        let (tx, rx) = oneshot::channel();
        {
            let mut waiters = self.waiters.lock().unwrap();
            let id =
                handler.request_opening_params(self.provider.node_id, self.provider.token.clone());
            waiters.insert(id, Waiter::Params(tx));
        }
        tokio::time::timeout(PROVIDER_ANSWER, rx)
            .await
            .map_err(|_| ())?
            .map_err(|_| ())
    }

    /// Buys the offer for this amount: the provider's answer is the short
    /// channel id and the CLTV delta for the invoice's route hint.
    async fn buy(&self, amount_msat: u64, offer: &Offer) -> Result<(u64, u32), InvoiceError> {
        let handler = self
            .liquidity
            .lsps2_client_handler()
            .ok_or_else(|| InvoiceError::Provider("no handler".into()))?;
        let (tx, rx) = oneshot::channel();
        {
            let mut waiters = self.waiters.lock().unwrap();
            let id = handler
                .select_opening_params(
                    self.provider.node_id,
                    Some(amount_msat),
                    offer.params.clone(),
                )
                .map_err(|e| InvoiceError::Provider(format!("{e:?}")))?;
            waiters.insert(id, Waiter::Buy(tx));
        }
        tokio::time::timeout(PROVIDER_ANSWER, rx)
            .await
            .map_err(|_| InvoiceError::Provider("no answer".into()))?
            .map_err(|_| InvoiceError::Provider("refused".into()))
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

/// Keeps one connection to the provider up. Nothing listens: the node takes
/// no inbound connection (SL-31).
async fn keep_provider_connected(shared: Arc<Shared>) {
    let addr: SocketAddr = shared.provider.addr;
    let id = shared.provider.node_id;
    loop {
        if shared.peers.peer_by_node_id(&id).is_none() {
            if let Some(connection) =
                lightning_net_tokio::connect_outbound(Arc::clone(&shared.peers), id, addr).await
            {
                tokio::spawn(connection);
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
        let LiquidityEvent::LSPS2Client(event) = event else {
            continue;
        };
        let mut waiters = shared.waiters.lock().unwrap();
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
            let allowed = claim == Claim::Allowed;
            match (preimage, allowed) {
                (Some(p), true) => shared.manager.claim_funds(p),
                _ => {
                    log(
                        Level::Warn,
                        format!("refused a payment of {amount_msat} msat, skimmed {counterparty_skimmed_fee_msat}: {claim:?}"),
                    );
                    shared.manager.fail_htlc_backwards(&payment_hash);
                }
            }
        }
        Event::PaymentClaimed {
            payment_hash,
            amount_msat,
            sender_intended_total_msat,
            ..
        } => {
            // Both copies hold the claim: the library gave this event only
            // after the monitor update was complete (SL-21).
            // Kept until the caller acknowledges it, and only then given: a
            // crash after this gives it again on the next start, and a
            // failure to keep it makes the library give the event again.
            let now = now_secs();
            let record = match shared
                .paid_log
                .record(PaidRecord {
                    payment_hash: payment_hash.0,
                    amount_msat,
                    sender_intended_msat: sender_intended_total_msat,
                    received_unix: now,
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
                let _ = shared.paid.send(Paid::from(record));
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
        }
        Event::ChannelClosed {
            channel_id, reason, ..
        } => {
            log(
                Level::Warn,
                format!("channel closed {channel_id}: {reason}"),
            );
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
    use crate::device::DirDevice;
    use crate::testutil::temp_dir;
    use lightning::routing::router::Router;
    use std::str::FromStr;

    fn config(network: Network) -> NodeConfig {
        let dir = temp_dir("node-config");
        NodeConfig {
            network,
            seed: NodeSeed([3; 64]),
            data_dir: dir.join("node"),
            // Nothing listens here: a start that gets as far as Core fails.
            core: CoreRpc {
                host: "127.0.0.1".into(),
                port: 1,
                user: "u".into(),
                password: "p".into(),
            },
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
            start_from: StartFrom::Newer,
            wallet: crate::testutil::wallet_text(),
            sweep_index: 0,
        }
    }

    #[test]
    fn sl_60_mainnet_is_refused_until_the_conditions_are_met() {
        const { assert!(!MAINNET_ALLOWED) };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt
            .block_on(Node::start(config(Network::Bitcoin)))
            .err()
            .unwrap();
        assert!(matches!(err, StartError::MainnetNotYet));
        assert_eq!(err.to_string(), crate::words::SL_60);
        // Refused before anything was made on disk.
        assert!(!temp_dir("node-config").join("node").exists());
    }

    #[test]
    fn sl_60_the_test_networks_get_past_the_refusal() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for n in [Network::Regtest, Network::Signet] {
            let err = rt.block_on(Node::start(config(n))).err().unwrap();
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
            let mut c = config(Network::Regtest);
            c.wallet = bad.to_string();
            let err = rt.block_on(Node::start(c)).err().unwrap();
            assert!(matches!(err, StartError::WalletRefused), "{err}");
            assert_eq!(err.to_string(), crate::words::SL_33_DESTINATION_REFUSED);
        }
        // An index with no address in the wallet is refused the same way.
        let mut c = config(Network::Regtest);
        c.sweep_index = 1 << 31;
        let err = rt.block_on(Node::start(c)).err().unwrap();
        assert!(matches!(err, StartError::WalletRefused), "{err}");
        assert!(!temp_dir("node-config").join("node").exists());
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
}
