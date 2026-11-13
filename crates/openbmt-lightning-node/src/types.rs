//! The concrete types the node is made of, and the two parts of the library
//! that stage 1 has nothing to put in: a router that finds no route (SL-01,
//! the node never pays) and a spawner on the tokio runtime.

use crate::chain::{ChainFilter, CoreBroadcaster, CoreFees};
use crate::logger::Logger;
use crate::store::DualStore;
use bitcoin::secp256k1::{PublicKey, Secp256k1};
use lightning::chain::chainmonitor::{AsyncPersister, ChainMonitor};
use lightning::ln::channel_state::ChannelDetails;
use lightning::ln::channelmanager::{ChannelManager, PaymentId};
use lightning::ln::msgs::RoutingMessageHandler;
use lightning::ln::peer_handler::{IgnoringMessageHandler, PeerManager};
use lightning::onion_message::messenger::{NullMessageRouter, OnionMessenger};
use lightning::routing::gossip::{NetworkGraph, P2PGossipSync};
use lightning::routing::router::{InFlightHtlcs, Route, RouteParameters, Router};
use lightning::routing::utxo::UtxoLookup;
use lightning::sign::{InMemorySigner, KeysManager};
use lightning::types::payment::PaymentHash;
use lightning::util::native_async::FutureSpawner;
use lightning::util::persist::MonitorUpdatingPersisterAsync;
use lightning_background_processor::GossipSync;
use lightning_liquidity::utils::time::DefaultTimeProvider;
use lightning_net_tokio::SocketDescriptor;
use lightning_rapid_gossip_sync::RapidGossipSync;
use std::future::Future;
use std::sync::Arc;

/// Runs the library's background futures on the tokio runtime.
pub struct TokioSpawner(pub tokio::runtime::Handle);

impl FutureSpawner for TokioSpawner {
    fn spawn<T: Future<Output = ()> + Send + 'static>(&self, future: T) {
        self.0.spawn(future);
    }
}

/// The router of a node that never pays (SL-01): there is no route to find.
pub struct NoRouter;

impl Router for NoRouter {
    fn find_route(
        &self,
        _payer: &PublicKey,
        _route_params: &RouteParameters,
        _first_hops: Option<&[&ChannelDetails]>,
        _inflight_htlcs: InFlightHtlcs,
    ) -> Result<Route, &'static str> {
        Err("this node does not pay")
    }

    fn find_route_with_id(
        &self,
        _payer: &PublicKey,
        _route_params: &RouteParameters,
        _first_hops: Option<&[&ChannelDetails]>,
        _inflight_htlcs: InFlightHtlcs,
        _payment_hash: PaymentHash,
        _payment_id: PaymentId,
    ) -> Result<Route, &'static str> {
        Err("this node does not pay")
    }

    fn create_blinded_payment_paths<
        T: bitcoin::secp256k1::Signing + bitcoin::secp256k1::Verification,
    >(
        &self,
        _recipient: PublicKey,
        _first_hops: Vec<ChannelDetails>,
        _tlvs: lightning::blinded_path::payment::ReceiveTlvs,
        _amount_msats: Option<u64>,
        _secp_ctx: &Secp256k1<T>,
    ) -> Result<Vec<lightning::blinded_path::payment::BlindedPaymentPath>, ()> {
        Err(())
    }
}

/// The persister of channel monitors: async, over the two-copy store.
pub type Persister = MonitorUpdatingPersisterAsync<
    Arc<DualStore>,
    TokioSpawner,
    Arc<Logger>,
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<CoreBroadcaster>,
    Arc<CoreFees>,
>;

/// What the chain monitor hands the async persister.
pub type MonitorPersister = AsyncPersister<
    Arc<DualStore>,
    TokioSpawner,
    Arc<Logger>,
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<CoreBroadcaster>,
    Arc<CoreFees>,
>;

/// The chain monitor.
pub type Monitor = ChainMonitor<
    InMemorySigner,
    Arc<ChainFilter>,
    Arc<CoreBroadcaster>,
    Arc<CoreFees>,
    Arc<Logger>,
    MonitorPersister,
    Arc<KeysManager>,
>;

/// The channel manager.
pub type Manager = ChannelManager<
    Arc<Monitor>,
    Arc<CoreBroadcaster>,
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<CoreFees>,
    Arc<NoRouter>,
    Arc<NullMessageRouter>,
    Arc<Logger>,
>;

/// The LSPS2 client.
pub type Liquidity = lightning_liquidity::LiquidityManager<
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<Manager>,
    Arc<ChainFilter>,
    Arc<DualStore>,
    DefaultTimeProvider,
    Arc<CoreBroadcaster>,
>;

/// The peer manager.
pub type Peers = PeerManager<
    SocketDescriptor,
    Arc<Manager>,
    Arc<IgnoringMessageHandler>,
    Arc<IgnoringMessageHandler>,
    Arc<Logger>,
    Arc<Liquidity>,
    Arc<KeysManager>,
    Arc<Monitor>,
>;

/// Only to name the types the background processor asks for; the node has no
/// onion messages.
pub type Onion = OnionMessenger<
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<Logger>,
    Arc<Manager>,
    Arc<NullMessageRouter>,
    Arc<Manager>,
    Arc<Manager>,
    IgnoringMessageHandler,
    IgnoringMessageHandler,
>;

/// Only to name the types the background processor asks for; the node learns
/// no gossip.
pub type Graph = NetworkGraph<Arc<Logger>>;
type Utxo = Arc<dyn UtxoLookup + Send + Sync>;
/// The gossip source of the background processor: none.
pub type NoGossip = GossipSync<
    Arc<P2PGossipSync<Arc<Graph>, Utxo, Arc<Logger>>>,
    Arc<RapidGossipSync<Arc<Graph>, Arc<Logger>>>,
    Arc<Graph>,
    Utxo,
    Arc<Logger>,
>;

// The background processor asks for a routing handler that is a trait object;
// the peer manager is given the ignoring one.
const _: fn() = || {
    fn is_routing<T: RoutingMessageHandler>() {}
    is_routing::<IgnoringMessageHandler>();
};
