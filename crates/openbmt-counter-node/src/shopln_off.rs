//! Shop Lightning where the node is not built (the `network` feature is off,
//! as in the browser demo). The types the counter hands over and reads keep
//! their shape; the node itself never starts, so the counter offers no
//! Lightning of its own and every call answers as a counter without it.

use std::net::SocketAddr;
use std::path::PathBuf;

use bitcoin::Network;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::relay::RelayHint;
use openbmt_core::settings::Settings;
use openbmt_counter::PaidFunding;
use openbmt_lightning::PaidFacts;
use openbmt_link::shopln::LightningReport;
use openbmt_net_io::DriverConfig;

use crate::lightning::SaleLightning;
use crate::lnshim::funding::FundingEvent;
use crate::shopln::{SeedNotTaken, Why};

/// What the app gives the counter at its start for Shop Lightning.
pub struct ShopLnConfig {
    /// The sealed seed record's bytes, as the app kept them, if it has them.
    pub seed_record: Option<Vec<u8>>,
    /// Makes the counter's identity, which seals and signs the Store
    /// messages (SL-74).
    pub identity: Box<dyn Fn() -> Option<Identity> + Send>,
    /// The relay of the pairing, which must be the Box named as the second
    /// device (SL-73).
    pub relay: Option<RelayHint>,
    /// The proxy the link to the relay goes through, if it needs one.
    pub link_proxy: Option<SocketAddr>,
}

/// What the node of the counter hands over for Shop Lightning at its start.
pub struct ShopLnArgs {
    /// This counter.
    pub me: DeviceId,
    /// The folder of the counter's data.
    pub dir: PathBuf,
    /// The network.
    pub network: Network,
    /// The driver's settings.
    pub driver: DriverConfig,
    /// Whether Tor is on in the settings (SL-32).
    pub tor_on: bool,
    /// From the app.
    pub config: ShopLnConfig,
}

/// Where Shop Lightning gets the shop's wallet and the addresses of its own
/// transactions (SL-33).
pub trait SweepSource {
    /// The description of the wallet the counter pays into now, if it holds
    /// one.
    fn wallet(&self) -> Option<String>;
    /// The next index of the ranges, taken as a sale takes its address.
    fn take_index(&mut self) -> Result<Option<u32>, String>;
}

/// A payment of Shop Lightning that the counter is to record.
pub struct PaidSale {
    /// The sale.
    pub sale: u32,
    /// What the counter is told, in the form its Lightning path takes.
    pub facts: PaidFacts,
    /// What is known of the fundings of the channels it came through.
    pub funding: Option<PaidFunding>,
}

/// What Home and the app read about Shop Lightning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopLnStatus {
    /// The app shows the screen that scans the seed (SL-76).
    pub needs_seed: bool,
    /// The seed is gone: the app deletes what it sealed (SL-77).
    pub delete_seed: bool,
    /// The reason Lightning is not offered, if it is not.
    pub why: Option<Why>,
    /// The sales paid through a channel whose funding was lost (SL-27).
    pub funding_lost: Vec<u32>,
}

/// Shop Lightning at the counter: here it never runs.
pub struct ShopLn;

impl ShopLn {
    /// A Shop Lightning that does not run.
    pub fn new(_args: ShopLnArgs) -> Self {
        Self
    }

    /// No lines.
    pub fn take_notes(&mut self) -> Vec<String> {
        Vec::new()
    }

    /// There is no seed.
    pub fn has_seed(&self) -> bool {
        false
    }

    /// The seed is not taken: the node is not here.
    pub fn take_seed(
        &mut self,
        _text: &str,
        _settings: &Settings,
        _now: u64,
        _src: &mut dyn SweepSource,
    ) -> Result<Vec<u8>, SeedNotTaken> {
        Err(SeedNotTaken::NotHere)
    }

    /// Nothing to reconcile.
    pub fn reconcile(&mut self, _settings: &Settings, _now: u64, _src: &mut dyn SweepSource) {}

    /// No Lightning for any sale.
    pub fn sale_view(&mut self, _index: u32, _sats: u64, _now: u64) -> SaleLightning {
        SaleLightning::Off
    }

    /// No invoice is shown, so none is final at once.
    pub fn final_at_once(&self, _index: u32) -> bool {
        false
    }

    /// Nothing to close.
    pub fn sale_closed(&mut self, _index: u32) {}

    /// Nothing to requote.
    pub fn sale_requoted(&mut self, _index: u32) {}

    /// No fundings.
    pub fn take_funding(&mut self) -> Vec<FundingEvent> {
        Vec::new()
    }

    /// No sale rests on a funding here.
    pub fn funding_lost(&mut self, _sales: &[u32]) -> bool {
        false
    }

    /// Nothing to hand back.
    pub fn funding_handled(&mut self, _txid: &[u8; 32]) {}

    /// No payments.
    pub fn take_paid(&mut self, _now: u64) -> Vec<PaidSale> {
        Vec::new()
    }

    /// No payment is kept.
    pub fn keep_unmatched(
        &mut self,
        _sale: Option<u32>,
        _hash: [u8; 32],
        _amount_msat: u64,
        _now: u64,
    ) -> bool {
        false
    }

    /// Nothing to mark.
    pub fn recorded(&mut self, _hash: &[u8; 32]) {}

    /// No report.
    pub fn report(&self) -> Option<LightningReport> {
        None
    }

    /// Lightning is not offered: the node is not here.
    pub fn status(&self) -> ShopLnStatus {
        ShopLnStatus {
            needs_seed: false,
            delete_seed: false,
            why: Some(Why::NotStarted(String::new())),
            funding_lost: Vec::new(),
        }
    }

    /// Nothing runs.
    pub fn stop(&mut self) {}
}
