//! The shop scenario: one counter of a shop that takes bitcoin, the owner's
//! books, a customer who pays, and the chain between them.
//!
//! Nothing here decides anything about a payment. The counter (`openbmt-counter`)
//! judges the sale and the books (`openbmt-books`) read the counter's signed
//! records, as the apps do; this module only plays the parts that the world
//! plays: the customer who pays, the network that relays and the miners.

use bitcoin::consensus::encode::serialize;
use bitcoin::Txid;
use openbmt_books::{format_minor, Books, Sale};
use openbmt_core::policy::Heartbeat;
use openbmt_core::records::{Chain, DeviceList, Record};
use openbmt_core::settings::Settings;
use openbmt_counter::{
    Amount, Counter, Currency, Facts, OpenFacts, Opened, Rate, RateSource, Screen,
};
use openbmt_watch::{Health, PeerAddr, Source, WellFormedBlock};

use crate::chain::{payment, SimChain};
use crate::pay::{payment_for, PaymentView};
use crate::rng::Rng;
use crate::store::MemStore;
use crate::wallet;

/// The first moment of the demo's clock: 1 January 2027, 00:00 UTC.
pub const START_TIME: u64 = 1_798_761_600;
/// Satoshis in one dollar at the demo's fixed rate of $100,000 a bitcoin.
pub const SATS_PER_USD: u64 = 1_000;
/// The demo's fixed rate, in cents per bitcoin.
pub const CENTS_PER_BTC: u64 = 10_000_000;
/// The name the demo's price carries where a source is named.
pub const RATE_SOURCE: &str = "demo-fixed";
/// The network groups that must report a payment for it to count as seen
/// (WR-44); the demo's network is that many peers, each in a group of its own.
const PEER_GROUPS: u8 = 3;

/// The state of the whole scenario.
#[derive(Debug)]
pub struct Shop {
    counter: Counter,
    store: MemStore,
    chain: SimChain,
    rng: Rng,
    now: u64,
    /// The sale on the counter's screen: its index and what opened it.
    sale: Option<Opened>,
    /// What the counter shows the customer for that sale.
    payment: Option<PaymentView>,
    /// The customer has paid the sale.
    paid: bool,
    /// What the sale asks, in cents.
    cents: u64,
    /// The id of the customer's payment for the sale on screen.
    ours: Option<Txid>,
}

/// The chain as the page draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainView {
    /// Each payment that waits: its satoshis and whether it is the sale's.
    pub pool: Vec<(u64, bool)>,
    /// Each block: its height, its payments and whether it holds the sale's.
    pub blocks: Vec<(u32, usize, bool)>,
}

/// What a step shows: the counter's screen, and the owner's view of the sale.
#[derive(Debug)]
pub struct Shown {
    /// The counter's screen for the sale, when there is one.
    pub screen: Option<Screen>,
    /// The owner's view of the sale from the books, when the books have it.
    pub owner: Option<OwnerSale>,
}

/// The sale as the owner's books have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerSale {
    /// The opaque sale ID (BK-07).
    pub id: String,
    /// The amount, as a decimal in the shop's currency.
    pub amount: String,
    /// The currency code.
    pub currency: String,
    /// The state, as the books name it.
    pub state: &'static str,
    /// Whether it counts as money in the books.
    pub counting: &'static str,
    /// The satoshis received, or why they are not known.
    pub sats_received: Result<u64, String>,
    /// The address.
    pub address: Option<String>,
}

fn refused<E: std::fmt::Debug>(what: &str, e: E) -> String {
    format!("{what}: {e:?}")
}

impl Shop {
    /// Set up the shop: the demo wallet, a fixed rate in dollars, and the
    /// counter paired with it. `seed` makes the customer's coins.
    pub fn set_up(seed: u64) -> Result<Self, String> {
        let pairing = wallet::pairing()?;
        let mut settings = Settings::defaults(SATS_PER_USD);
        settings.customer_wifi = false;
        settings.min_sale_sats = 0;
        let me = wallet::counter()?.device_id();
        settings.devices.insert(me);
        settings.counters.insert(me, 10_000);
        let counter = Counter::new_own(wallet::counter()?, &pairing, settings)
            .map_err(|e| refused("the counter did not start", e))?;
        Ok(Self {
            counter,
            store: MemStore::default(),
            chain: SimChain::new(START_TIME),
            rng: Rng::new(seed),
            now: START_TIME,
            sale: None,
            payment: None,
            paid: false,
            cents: 0,
            ours: None,
        })
    }

    /// The time on the demo's clock, in seconds since 1970.
    pub fn now(&self) -> u64 {
        self.now
    }

    /// The height of the chain's tip.
    pub fn height(&self) -> u32 {
        use openbmt_watch::HeaderChain;
        self.chain.tip_height()
    }

    /// How many transactions wait in the mempool.
    pub fn mempool(&self) -> usize {
        self.chain.mempool_len()
    }

    /// The chain as the page draws it: the payments that wait (satoshis, and
    /// whether it is the sale's) and the blocks (height, payments, and whether
    /// one is the sale's).
    pub fn chain_view(&self) -> ChainView {
        let ours = self.ours;
        ChainView {
            pool: self
                .chain
                .pool()
                .into_iter()
                .map(|(id, sats)| (sats, Some(id) == ours))
                .collect(),
            blocks: self
                .chain
                .blocks()
                .iter()
                .map(|b| {
                    (
                        b.height,
                        b.txids.len(),
                        ours.is_some_and(|o| b.txids.contains(&o)),
                    )
                })
                .collect(),
        }
    }

    /// What the counter shows the customer for the sale on screen.
    pub fn payment(&self) -> Option<&PaymentView> {
        self.payment.as_ref()
    }

    /// The sale on screen.
    pub fn sale(&self) -> Option<&Opened> {
        self.sale.as_ref()
    }

    /// What the sale on screen asks, in cents.
    pub fn cents(&self) -> u64 {
        self.cents
    }

    /// Whether the customer has paid the sale on screen.
    pub fn customer_paid(&self) -> bool {
        self.paid
    }

    fn health() -> Health {
        Health {
            healthy: true,
            problem: None,
            behind: false,
        }
    }

    /// The sale's screen as the counter judges it now.
    fn judge(&mut self) -> Result<Option<Screen>, String> {
        let Some(index) = self.sale.as_ref().map(|s| s.index) else {
            return Ok(None);
        };
        let facts = Facts {
            chain: &self.chain,
            health: Self::health(),
            heartbeat: Heartbeat {
                verified: true,
                age_secs: 0,
            },
            remote_order: false,
            now: self.now,
        };
        self.counter
            .judge(index, &facts, &mut self.store)
            .map(Some)
            .map_err(|e| refused("the counter could not judge the sale", e))
    }

    /// A new sale of `cents`: the counter gives it an address from the demo
    /// wallet, and the screen shows the code the customer scans.
    pub fn new_sale(&mut self, cents: u64) -> Result<Shown, String> {
        let facts = OpenFacts {
            health: Self::health(),
            rescan_pending: false,
            now: self.now,
        };
        let currency = Currency::of_shop(self.counter.settings().currency);
        let rate = Rate::new(CENTS_PER_BTC).ok_or("the demo rate is not a rate")?;
        let source = RateSource::new(RATE_SOURCE, self.now).map_err(|e| refused("rate", e))?;
        let opened = self
            .counter
            .open(
                Amount(cents),
                currency,
                rate,
                &source,
                facts,
                &mut self.store,
            )
            .map_err(|e| refused("the sale was not opened", e))?;
        self.payment = Some(payment_for(&opened.script, opened.needs_sats)?);
        self.sale = Some(opened);
        self.paid = false;
        self.cents = cents;
        self.ours = None;
        self.shown()
    }

    /// The customer pays: a transaction that pays the sale's address the
    /// amount asked goes into the mempool, and the network, three peers of
    /// different groups, tells the counter about it.
    pub fn customer_pays(&mut self) -> Result<Shown, String> {
        let opened = self.sale.as_ref().ok_or("there is no sale")?;
        if self.paid {
            return Err("the customer has paid this sale".to_string());
        }
        let tx = payment(self.rng.next_32(), &opened.script, opened.needs_sats);
        let bytes = serialize(&tx);
        self.ours = Some(tx.compute_txid());
        self.chain.broadcast(tx);
        self.paid = true;
        for n in 20..20 + PEER_GROUPS {
            let source = peer(n)?;
            self.counter
                .sighting(&bytes, source, self.now, &mut self.store)
                .map_err(|e| refused("the counter refused the payment", e))?;
        }
        self.shown()
    }

    /// Let `secs` seconds pass.
    pub fn tick(&mut self, secs: u64) -> Result<Shown, String> {
        self.now = self.now.saturating_add(secs);
        self.shown()
    }

    /// Mine a block of the mempool, ten minutes after the last moment, and
    /// tell the counter about it.
    pub fn mine(&mut self) -> Result<Shown, String> {
        self.now = self.now.saturating_add(600);
        let block = self.chain.mine(self.now);
        let wf = WellFormedBlock::parse(&serialize(&block))
            .map_err(|e| refused("the block was refused", e))?;
        self.counter
            .block(&wf, &self.chain, self.now, &mut self.store)
            .map_err(|e| refused("the counter refused the block", e))?;
        self.shown()
    }

    /// What the screens show now.
    pub fn shown(&mut self) -> Result<Shown, String> {
        let screen = self.judge()?;
        let owner = self.owner_view()?;
        Ok(Shown { screen, owner })
    }

    /// The owner's books, built from the counter's signed records as the
    /// owner app builds them.
    fn books(&self) -> Result<Books, String> {
        let me = wallet::counter()?.device_id();
        let mut records = Vec::with_capacity(self.store.records.len());
        for bytes in &self.store.records {
            records.push(Record::from_bytes(bytes).map_err(|e| refused("a record", e))?);
        }
        let chain = Chain::from_records(records);
        Books::from_chains(&DeviceList::new([me]), &[(me, &chain)])
            .map_err(|e| refused("the books", e))
    }

    fn owner_view(&self) -> Result<Option<OwnerSale>, String> {
        let Some(index) = self.sale.as_ref().map(|s| s.index) else {
            return Ok(None);
        };
        let books = self.books()?;
        Ok(books
            .sales()
            .iter()
            .find(|s| s.key.index == index)
            .map(owner_sale))
    }
}

fn owner_sale(s: &Sale) -> OwnerSale {
    OwnerSale {
        id: s.id.to_string(),
        amount: format_minor(s.amount_minor, s.currency),
        currency: s.currency.to_string(),
        state: s.state.text(),
        counting: s.counting().text(),
        sats_received: match s.sats_received.known() {
            Some(sats) => Ok(*sats),
            None => Err(s.sats_received.unknown().map_or_else(String::new, |u| {
                let u = u.explain();
                format!("{} {}", u.why, u.next_step)
            })),
        },
        address: s.address.known().cloned(),
    }
}

/// A peer in a group of its own for each `n`.
fn peer(n: u8) -> Result<Source, String> {
    format!("{n}.{n}.1.1")
        .parse()
        .map(|ip| Source::Peer(PeerAddr::Ip(ip)))
        .map_err(|e| refused("a peer", e))
}
