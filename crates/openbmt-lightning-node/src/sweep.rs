//! Stage 1 has no output sweeper and no scorer. The types here only name what
//! the library's background loop is told is not there.

use crate::chain::{ChainFilter, CoreBroadcaster, CoreFees};
use crate::logger::Logger;
use crate::store::DualStore;
use crate::types::Graph;
use bitcoin::ScriptBuf;
use lightning::routing::scoring::ProbabilisticScorer;
use lightning::sign::{ChangeDestinationSource, KeysManager};
use lightning::util::sweep::OutputSweeper;
use std::sync::Arc;

/// Where a sweeper would send its outputs: nowhere.
pub struct NoChange;

impl ChangeDestinationSource for NoChange {
    fn get_change_destination_script<'a>(
        &'a self,
    ) -> lightning::util::async_poll::AsyncResult<'a, ScriptBuf, ()> {
        Box::pin(async { Err(()) })
    }
}

/// The sweeper type, to be absent.
pub type NoSweeper = OutputSweeper<
    Arc<CoreBroadcaster>,
    Arc<NoChange>,
    Arc<CoreFees>,
    Arc<ChainFilter>,
    Arc<DualStore>,
    Arc<Logger>,
    Arc<KeysManager>,
>;

/// The scorer type, to be absent.
pub type NoScorer = std::sync::Mutex<ProbabilisticScorer<Arc<Graph>, Arc<Logger>>>;
