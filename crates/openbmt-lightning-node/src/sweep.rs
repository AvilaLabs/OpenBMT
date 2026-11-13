//! Spendable outputs are swept (SL-33's money from a close, SL-36, SL-24).
//!
//! When a channel is closed on the chain, the library gives the node outputs
//! it can spend. `lightning::util::sweep::OutputSweeper` tracks them, spends
//! them to the address the caller gave and keeps its state in the store, the
//! way the manager is kept: the counter's copy at once and the second
//! device's as soon as it can be reached, the latest write of a name winning
//! (SL-24).
//!
//! The address is the shop wallet's next address, given in the node's
//! configuration as a script and changed by the caller with
//! [`SweepDestination::set`]. What was set last is kept in the store and used
//! after a restart.

use crate::chain::{ChainFilter, CoreBroadcaster, CoreFees};
use crate::logger::Logger;
use crate::store::Background;
use crate::types::Graph;
use bitcoin::ScriptBuf;
use lightning::chain::BestBlock;
use lightning::io;
use lightning::routing::scoring::ProbabilisticScorer;
use lightning::sign::{
    ChangeDestinationSource, KeysManager, SignerProvider, SpendableOutputDescriptor,
};
use lightning::util::persist::{
    KVStore, OUTPUT_SWEEPER_PERSISTENCE_KEY, OUTPUT_SWEEPER_PERSISTENCE_PRIMARY_NAMESPACE,
    OUTPUT_SWEEPER_PERSISTENCE_SECONDARY_NAMESPACE,
};
use lightning::util::ser::ReadableArgs;
use lightning::util::sweep::OutputSweeper;
use std::sync::{Arc, Mutex};

/// Where the destination script is kept.
pub const DESTINATION_PRIMARY: &str = "sweep_destination";
const DESTINATION_KEY: &str = "script";

/// Where the sweeper sends what it spends: the shop wallet's address, kept in
/// the store.
pub struct SweepDestination {
    script: Mutex<ScriptBuf>,
    store: Arc<Background>,
}

impl SweepDestination {
    /// The destination after a start: the one the caller set last if there is
    /// one in the store, otherwise `configured`, which is then kept.
    pub async fn load(store: Arc<Background>, configured: ScriptBuf) -> Result<Self, io::Error> {
        let kept = match KVStore::read(&*store, DESTINATION_PRIMARY, "", DESTINATION_KEY).await {
            Ok(bytes) if !bytes.is_empty() => Some(ScriptBuf::from_bytes(bytes)),
            Ok(_) => None,
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let me = SweepDestination {
            script: Mutex::new(kept.clone().unwrap_or_else(|| configured.clone())),
            store,
        };
        if kept.is_none() {
            me.keep(&configured).await?;
        }
        Ok(me)
    }

    async fn keep(&self, script: &ScriptBuf) -> Result<(), io::Error> {
        KVStore::write(
            &*self.store,
            DESTINATION_PRIMARY,
            "",
            DESTINATION_KEY,
            script.to_bytes(),
        )
        .await
    }

    /// Changes the destination. An empty script is refused. Returns when the
    /// counter's copy holds it; a sweep that comes after uses it.
    pub async fn set(&self, script: ScriptBuf) -> Result<(), io::Error> {
        if script.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "an empty script is not a destination",
            ));
        }
        self.keep(&script).await?;
        *self.script.lock().unwrap() = script;
        Ok(())
    }

    /// The destination now.
    pub fn get(&self) -> ScriptBuf {
        self.script.lock().unwrap().clone()
    }
}

impl ChangeDestinationSource for SweepDestination {
    fn get_change_destination_script<'a>(
        &'a self,
    ) -> lightning::util::async_poll::AsyncResult<'a, ScriptBuf, ()> {
        let script = self.get();
        Box::pin(async move { Ok(script) })
    }
}

/// The sweeper.
pub type Sweeper = OutputSweeper<
    Arc<CoreBroadcaster>,
    Arc<SweepDestination>,
    Arc<CoreFees>,
    Arc<ChainFilter>,
    Arc<Background>,
    Arc<Logger>,
    Arc<KeysManager>,
>;

/// What the sweeper is made from.
#[derive(Clone)]
pub struct SweeperParts {
    /// Sends the spending transactions.
    pub broadcaster: Arc<CoreBroadcaster>,
    /// The fee rate: the node's own, from Core.
    pub fees: Arc<CoreFees>,
    /// Told of the outputs to watch.
    pub filter: Arc<ChainFilter>,
    /// Where the sweeper's state is kept.
    pub store: Arc<Background>,
    /// The log.
    pub logger: Arc<Logger>,
    /// Signs the spends.
    pub keys: Arc<KeysManager>,
    /// Where the money goes.
    pub destination: Arc<SweepDestination>,
}

impl SweeperParts {
    /// The sweeper as the store last had it, with the best block it had
    /// reached, or none if nothing was kept.
    pub async fn read(&self) -> Result<Option<(BestBlock, Arc<Sweeper>)>, io::Error> {
        let bytes = match KVStore::read(
            &*self.store,
            OUTPUT_SWEEPER_PERSISTENCE_PRIMARY_NAMESPACE,
            OUTPUT_SWEEPER_PERSISTENCE_SECONDARY_NAMESPACE,
            OUTPUT_SWEEPER_PERSISTENCE_KEY,
        )
        .await
        {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let (best, sweeper) = <(BestBlock, Sweeper)>::read(
            &mut io::Cursor::new(bytes),
            (
                Arc::clone(&self.broadcaster),
                Arc::clone(&self.fees),
                Some(Arc::clone(&self.filter)),
                Arc::clone(&self.keys),
                Arc::clone(&self.destination),
                Arc::clone(&self.store),
                Arc::clone(&self.logger),
            ),
        )
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("sweeper: {e:?}")))?;
        Ok(Some((best, Arc::new(sweeper))))
    }

    /// A sweeper with nothing tracked, at `best_block`.
    pub fn new_at(&self, best_block: BestBlock) -> Arc<Sweeper> {
        Arc::new(OutputSweeper::new(
            best_block,
            Arc::clone(&self.broadcaster),
            Arc::clone(&self.fees),
            Some(Arc::clone(&self.filter)),
            Arc::clone(&self.keys),
            Arc::clone(&self.destination),
            Arc::clone(&self.store),
            Arc::clone(&self.logger),
        ))
    }
}

/// Whether the node's own keys can spend this output. A static output that pays
/// to the shop's wallet (the address a close was given, SL-36) is already in the
/// wallet and is not the sweeper's: tracked, it would make every sweep that
/// carries it fail. Every other kind of descriptor is made by the node's own
/// channel keys.
pub fn spendable_by_node(keys: &KeysManager, descriptor: &SpendableOutputDescriptor) -> bool {
    match descriptor {
        SpendableOutputDescriptor::StaticOutput { output, .. } => {
            let own_destination = keys.get_destination_script([0; 32]).ok();
            let own_shutdown = keys
                .get_shutdown_scriptpubkey()
                .ok()
                .map(|s| s.into_inner());
            Some(&output.script_pubkey) == own_destination.as_ref()
                || Some(&output.script_pubkey) == own_shutdown.as_ref()
        }
        _ => true,
    }
}

/// The scorer type, to be absent: the node never pays.
pub type NoScorer = std::sync::Mutex<ProbabilisticScorer<Arc<Graph>, Arc<Logger>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bip85::NodeSeed;
    use crate::chain::CoreRpc;
    use crate::device::{DeviceStatus, DirDevice, GatedDevice, SecondDevice};
    use crate::seal::{open, SealKey};
    use crate::store::{device_name, DualStore};
    use crate::testutil::temp_dir;
    use bitcoin::hashes::Hash;
    use bitcoin::{Amount, TxOut, Txid};
    use lightning::chain::transaction::OutPoint;
    use lightning::sign::SpendableOutputDescriptor;
    use lightning::util::logger::Level;
    use lightning_persister::fs_store::FilesystemStore;
    use std::time::Duration;

    struct Disks {
        dir: std::path::PathBuf,
        device: GatedDevice<DirDevice>,
        key: SealKey,
    }

    fn disks(name: &str) -> Disks {
        let dir = temp_dir(name);
        Disks {
            device: GatedDevice::new(DirDevice::new(dir.join("device")).unwrap()),
            key: SealKey::from_seed(&NodeSeed([5; 64])),
            dir,
        }
    }

    /// The background store of a start over the same disks, with its flusher.
    fn start(d: &Disks) -> (Arc<Background>, tokio::task::JoinHandle<()>) {
        let store = Arc::new(DualStore::new(
            Arc::new(FilesystemStore::new(d.dir.join("local"))),
            Arc::new(d.device.clone()),
            d.key.clone(),
            Arc::new(KeysManager::new(&[1; 32], 1, 1, true)),
            Arc::new(DeviceStatus::new()),
            Arc::new(Logger::open(&d.dir.join("log"), Level::Debug).unwrap()),
        ));
        let flusher = store.spawn_lazy_flusher(&tokio::runtime::Handle::current());
        (Arc::new(Background(store)), flusher)
    }

    fn script(n: u8) -> ScriptBuf {
        let mut b = vec![0x00, 0x14];
        b.extend_from_slice(&[n; 20]);
        ScriptBuf::from_bytes(b)
    }

    #[tokio::test]
    async fn sl_33_the_sweep_destination_is_the_configured_one_until_it_is_changed_and_then_the_changed_one(
    ) {
        let d = disks("sweep-dest");
        let (store, flusher) = start(&d);
        let dest = SweepDestination::load(Arc::clone(&store), script(1))
            .await
            .unwrap();
        assert_eq!(dest.get(), script(1));
        assert_eq!(
            dest.get_change_destination_script().await.unwrap(),
            script(1)
        );
        dest.set(script(2)).await.unwrap();
        assert_eq!(
            dest.get_change_destination_script().await.unwrap(),
            script(2)
        );
        assert!(dest.set(ScriptBuf::new()).await.is_err());
        assert_eq!(dest.get(), script(2));
        // A restart with the first configured script in the config again: what
        // the caller set last is kept.
        flusher.abort();
        let (store, _flusher) = start(&d);
        let again = SweepDestination::load(store, script(1)).await.unwrap();
        assert_eq!(again.get(), script(2));
    }

    #[tokio::test]
    async fn sl_33_a_first_start_keeps_the_configured_destination() {
        let d = disks("sweep-dest-first");
        let (store, flusher) = start(&d);
        SweepDestination::load(store, script(7)).await.unwrap();
        flusher.abort();
        let kept = std::fs::read(d.dir.join("local/sweep_destination/script")).unwrap();
        assert_eq!(kept, script(7).to_bytes());
    }

    async fn parts(d: &Disks) -> (SweeperParts, tokio::task::JoinHandle<()>) {
        let (store, flusher) = start(d);
        let logger = Arc::new(Logger::open(&d.dir.join("sweep-log"), Level::Debug).unwrap());
        let rpc = Arc::new(
            CoreRpc {
                host: "127.0.0.1".into(),
                port: 1,
                user: "u".into(),
                password: "p".into(),
            }
            .client(),
        );
        let parts = SweeperParts {
            broadcaster: Arc::new(CoreBroadcaster::new(
                rpc,
                tokio::runtime::Handle::current(),
                Arc::clone(&logger),
            )),
            fees: Arc::new(CoreFees::default()),
            filter: Arc::new(ChainFilter::default()),
            keys: Arc::new(KeysManager::new(&[1; 32], 1, 1, true)),
            destination: Arc::new(
                SweepDestination::load(Arc::clone(&store), script(1))
                    .await
                    .unwrap(),
            ),
            store,
            logger,
        };
        (parts, flusher)
    }

    #[tokio::test]
    async fn sl_24_the_sweepers_state_is_kept_local_first_and_comes_back_after_a_restart() {
        let d = disks("sweep-state");
        let (p, flusher) = parts(&d).await;
        let best = BestBlock::from_network(bitcoin::Network::Regtest);
        let sweeper = p.new_at(best);
        let descriptor = SpendableOutputDescriptor::StaticOutput {
            outpoint: OutPoint {
                txid: Txid::from_byte_array([9; 32]),
                index: 1,
            },
            output: TxOut {
                value: Amount::from_sat(40_000),
                script_pubkey: script(3),
            },
            channel_keys_id: None,
        };
        // Static outputs are kept too: the node has no wallet that would spend them.
        sweeper
            .track_spendable_outputs(vec![descriptor.clone()], None, false, None)
            .await
            .unwrap();
        assert_eq!(sweeper.tracked_spendable_outputs().len(), 1);
        // On the counter's disk at once.
        assert!(d.dir.join("local/output_sweeper").exists());
        // On the second device when the flusher has run.
        let name = device_name("", "", "output_sweeper");
        let mut sealed = None;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            sealed = d.device.read(&name).await.unwrap();
            if sealed.is_some() {
                break;
            }
        }
        assert!(open(&d.key, &name, &sealed.unwrap()).is_ok());
        // A restart: read back, with the output tracked and the best block kept.
        flusher.abort();
        let (p2, _flusher) = parts(&d).await;
        let (best_back, again) = p2.read().await.unwrap().unwrap();
        assert_eq!(best_back, best);
        let tracked = again.tracked_spendable_outputs();
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].descriptor, descriptor);
        // A second track of the same output is not tracked twice.
        again
            .track_spendable_outputs(vec![descriptor], None, false, None)
            .await
            .unwrap();
        assert_eq!(again.tracked_spendable_outputs().len(), 1);
    }

    #[test]
    fn sl_33_a_static_output_to_the_shops_wallet_is_not_the_sweepers() {
        let keys = KeysManager::new(&[1; 32], 1, 1, true);
        let out = |script_pubkey| SpendableOutputDescriptor::StaticOutput {
            outpoint: lightning::chain::transaction::OutPoint {
                txid: Txid::from_byte_array([1; 32]),
                index: 0,
            },
            output: TxOut {
                value: Amount::from_sat(10_000),
                script_pubkey,
            },
            channel_keys_id: None,
        };
        // The wallet's address, which a close was given: not tracked.
        assert!(!spendable_by_node(&keys, &out(script(5))));
        // The node's own destination and shutdown scripts: tracked.
        assert!(spendable_by_node(
            &keys,
            &out(keys.get_destination_script([0; 32]).unwrap())
        ));
        assert!(spendable_by_node(
            &keys,
            &out(keys.get_shutdown_scriptpubkey().unwrap().into_inner())
        ));
    }

    #[tokio::test]
    async fn sl_24_a_start_with_no_sweeper_state_reads_none() {
        let d = disks("sweep-none");
        let (p, _f) = parts(&d).await;
        assert!(p.read().await.unwrap().is_none());
    }
}
