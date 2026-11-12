//! The owner phone of a shop with its own wallet and a counter, both real
//! (`docs/OWNER-APP-RULES.md` OA-105 to OA-112, `docs/COUNTER-RULES.md`
//! CR-227 to CR-235): the owner's `OwnNode` makes the offer, reads the
//! counter's request, makes the bundle and, on the link, the top-up range;
//! the counter is the real node behind `dispatch`, on the loopback relay.
//! Nothing of the owner's side is made by hand.

use std::net::TcpListener;
use std::sync::mpsc::channel;
use std::sync::Arc;

use bitcoin::Network;
use openbmt_core::identity::Identity;
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_core::records::DeviceList;
use openbmt_core::relay::{RelayHint, RelayRoute, SignedRelayHint};
use openbmt_counter_node::DriverPort;
use openbmt_courier::{Courier, CourierConfig, FirstStart, RelayPost};
use openbmt_link::{
    ClientConfig, MemoryStore, Relay, RelayConfig, Role, Route, Server, ServerConfig,
};
use openbmt_net_io::{AppEvent, DriverConfig, EventReceiver, SystemClock};
use openbmt_owner::own_shop::MadeOffer;
use openbmt_owner_node::{OwnNode, OwnerConfig, OwnerLink, OwnerLinkConfig, OwnerWatcher};

use super::kit::{peers_event, Rig, Scratch, FIVE_DOLLARS, RATE, T0};
use super::pairing_own::{begin, finish, matched, same, MULTI, SINGLE};
use super::wire::{Rd, W};
use crate::StartConfig;

const OP_PUMP: u16 = 10;
const OP_OPEN: u16 = 11;
const OP_LINK_STEP: u16 = 24;
/// Seconds between two sales: the counter hands out 120 addresses an hour
/// at most (CR-04).
const SALE_GAP: u64 = 31;

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

/// A driver that holds nothing and gives no event: the owner's link talks
/// to the relay and never to the chain in this test.
struct IdlePort(EventReceiver);

impl DriverPort for IdlePort {
    fn events(&self) -> &EventReceiver {
        &self.0
    }
    fn watch_script(&self, _: bitcoin::ScriptBuf) {}
    fn set_all_blocks(&self, _: bool) {}
    fn shutdown(&mut self) {}
}

/// The owner phone as the app has it: the node, the folder, the relay
/// hint and the owner's end of the link once a counter is paired.
struct Owner {
    node: OwnNode,
    _dir: Scratch,
    link_dir: Scratch,
    watcher_dir: Scratch,
    listener: Option<TcpListener>,
    server: Option<Server>,
    link: Option<OwnerLink<RelayPost>>,
    watcher: Option<OwnerWatcher<IdlePort>>,
    _events: Option<std::sync::mpsc::Sender<AppEvent>>,
}

impl Owner {
    fn new(text: &str) -> Self {
        let dir = Scratch::new();
        let node = OwnNode::open(dir.path(), id(1), PinnedWallet::from_text(text).unwrap())
            .expect("the owner node opens");
        Self {
            node,
            _dir: dir,
            link_dir: Scratch::new(),
            watcher_dir: Scratch::new(),
            listener: Some(TcpListener::bind("127.0.0.1:0").unwrap()),
            server: None,
            link: None,
            watcher: None,
            _events: None,
        }
    }

    fn hint(&self) -> SignedRelayHint {
        let addr = self.listener.as_ref().unwrap().local_addr().unwrap();
        let hint = RelayHint::new(RelayRoute::Direct(addr), id(99).device_id().to_bytes()).unwrap();
        SignedRelayHint::sign(hint, &id(1))
    }

    /// OA-108, OA-109, OA-110: the offer's code, the counter's request read,
    /// and the bundle after "They match".
    fn offer(&self, now: u64) -> MadeOffer {
        self.node.shop().make_offer(now).expect("an offer")
    }

    fn confirm(
        &mut self,
        made: &MadeOffer,
        now: u64,
        request: &[u8],
    ) -> openbmt_owner::own_shop::Paired {
        let relay = Some(self.hint());
        let (shop, store) = self.node.shop_and_store();
        shop.confirm(made, now, request, 100_000, Vec::new(), relay, &store)
            .expect("they match")
    }

    /// The relay with the owner and the paired counter as members, and the
    /// owner's end of the link.
    fn start_link(&mut self, counter: openbmt_core::identity::DeviceId) {
        let mut relay = Relay::new(id(99), RelayConfig::default(), MemoryStore::new()).unwrap();
        relay.set_members(&[id(1).device_id(), counter], 0).unwrap();
        let listener = self.listener.take().unwrap();
        let addr = listener.local_addr().unwrap();
        self.server = Some(
            Server::start(
                listener,
                relay,
                Arc::new(SystemClock),
                ServerConfig::default(),
            )
            .unwrap(),
        );
        let courier = Courier::start(
            CourierConfig {
                dir: self.link_dir.path().to_path_buf(),
                identity: id(1),
                role: Role::Owner,
                peers: [(counter, Role::Counter)].into_iter().collect(),
                route: Route::Direct(addr),
                relay: id(99).device_id(),
                client: ClientConfig::default(),
            },
            FirstStart::Yes,
        )
        .unwrap();
        let config = OwnerLinkConfig {
            poll_secs: 0,
            retry_secs: 0,
            ..OwnerLinkConfig::default()
        };
        self.link = Some(
            OwnerLink::new(
                courier,
                self.link_dir.path(),
                DeviceList::new([counter, id(1).device_id()]),
                vec![counter],
                config,
            )
            .unwrap(),
        );
        let (tx, rx) = channel();
        let watcher = OwnerWatcher::start_with(
            OwnerConfig::new(
                self.watcher_dir.path(),
                id(1),
                DriverConfig::new(Network::Regtest, 0),
            ),
            move |_, _| Ok(IdlePort(rx.into())),
        )
        .unwrap();
        self._events = Some(tx);
        self.watcher = Some(watcher);
    }

    fn step(&mut self, now: u64) {
        let link = self.link.as_mut().unwrap();
        link.step(self.watcher.as_mut().unwrap(), now);
    }
}

/// A turn of both ends of the link: the counter's step, then the owner's.
fn turn(rig: &Rig, handle: u64, owner: &mut Owner, now: u64) {
    let response = rig.call(OP_LINK_STEP, handle, &W::new().u64(now).u8(0).0);
    let mut r = Rd::ok(&response);
    let _ = r.link();
    let _ = r.list(Rd::string);
    r.done();
    owner.step(now);
}

/// Turns until the owner's copy of the counter's chain stops growing.
fn settle(rig: &Rig, handle: u64, owner: &mut Owner, mut now: u64) -> u64 {
    let counter = owner.link.as_ref().unwrap().counters()[0];
    let mut last = 0;
    let mut quiet = 0;
    for _ in 0..40 {
        now += 10;
        turn(rig, handle, owner, now);
        let len = owner.link.as_ref().unwrap().copy(&counter).unwrap().len();
        if len == last {
            quiet += 1;
            if quiet >= 3 {
                break;
            }
        } else {
            quiet = 0;
            last = len;
        }
    }
    now
}

fn open_sale(rig: &Rig, handle: u64, now: u64) -> super::wire::Opened {
    let response = rig.call(
        OP_OPEN,
        handle,
        &W::new()
            .open(now, FIVE_DOLLARS, "USD", RATE, "test-fixed")
            .0,
    );
    Rd::ok(&response).opened()
}

#[test]
fn the_real_owner_pairs_the_real_counter_which_sells_from_the_wallet_and_takes_a_top_up() {
    for text in [SINGLE, MULTI] {
        let wallet = PinnedWallet::from_text(text).unwrap();
        let mut owner = Owner::new(text);

        // OA-108: the offer. The counter reads it (CR-228) and, after
        // "Same address", shows its request and four words (CR-229).
        let made = owner.offer(T0);
        let rig = Rig::new();
        let (_, _, address, _) = begin(&rig, &made.offer.to_bytes()).expect("the counter reads it");
        assert_eq!(address, wallet.receive_address(0).unwrap().to_string());
        let (request, counter_words) = same(&rig, T0 + 10).expect("same address");

        // OA-109: the owner reads the request, and shows the same words.
        let read = owner
            .node
            .shop()
            .read_request(&made, T0 + 20, &request)
            .expect("the request reads");
        assert_eq!(
            read.words.iter().map(|w| w.to_string()).collect::<Vec<_>>(),
            counter_words
        );
        assert_eq!(read.name, "Front counter");

        // OA-110: "They match" on both phones, then the bundle.
        matched(&rig, T0 + 30).expect("they match on the counter");
        let paired = owner.confirm(&made, T0 + 30, &request);
        assert_eq!(
            (paired.first_range.start, paired.first_range.end),
            (0, 1_000)
        );
        let record = finish(&rig, &paired.bundle.to_bytes(), T0 + 40).expect("the counter pairs");

        // OA-111: the bundle waits until the owner says Paired.
        assert_eq!(owner.node.shop().counters_waiting().len(), 1);
        {
            let (shop, store) = owner.node.shop_and_store();
            shop.mark_delivered(&read.counter, &store).unwrap();
        }
        assert!(owner.node.shop().counters_waiting().is_empty());

        // The counter starts from its record and opens a sale at receive
        // index 0: the wallet's first address (CR-234).
        owner.start_link(read.counter);
        let handle = rig.start_with(&StartConfig {
            dir: rig.dir().to_str().unwrap().to_string(),
            now: T0 + 100,
            record,
            proxy: None,
            own_node: None,
            lightning: None,
            link_proxy: None,
        });
        rig.send(vec![peers_event()]);
        let response = rig.call(OP_PUMP, handle, &W::new().u64(T0 + 100).u32(50).0);
        Rd::ok(&response);
        let first = open_sale(&rig, handle, T0 + 100);
        assert_eq!(first.index, 0);
        assert_eq!(
            first.payment.address,
            wallet.receive_address(0).unwrap().to_string()
        );

        // OA-107: the owner watches every index of the range.
        assert_eq!(owner.node.watch_scripts().unwrap().len(), 1_000);

        // 720 sales in all leave 280 of the 1,000 indexes unused. The owner
        // reads the counter's records and gives it the next range.
        let mut now = T0 + 100;
        for _ in 1..720 {
            now += SALE_GAP;
            let opened = open_sale(&rig, handle, now);
            assert!(opened.index < 1_000);
        }
        now = settle(&rig, handle, &mut owner, now);
        let counter = read.counter;
        let link = owner.link.as_mut().unwrap();
        assert_eq!(owner.node.unused_of(link, &counter), Some(280));
        let topped = owner.node.top_up(link, now).expect("a top-up");
        assert_eq!(topped.len(), 1);
        assert_eq!((topped[0].start, topped[0].end), (1_000, 2_000));
        assert!(link.ranges_waiting(&counter).is_some());
        assert_eq!(
            owner.node.watch_scripts().unwrap().len(),
            2_000,
            "the new range is watched at once"
        );

        // The counter takes it from the link; its record of it shows in the
        // owner's copy, and the range no longer waits (OA-112).
        now = settle(&rig, handle, &mut owner, now);
        let link = owner.link.as_mut().unwrap();
        assert!(
            link.ranges_waiting(&counter).is_none(),
            "the range was not shown as held"
        );
        assert_eq!(owner.node.unused_of(link, &counter), Some(1_280));
        assert!(owner.node.top_up(link, now + 1).unwrap().is_empty());

        // The counter sells on through 999 and then from 1,000: the first
        // index of the range it was sent.
        for expected in 720..1_000 {
            now += SALE_GAP;
            assert_eq!(open_sale(&rig, handle, now).index, expected);
        }
        now += SALE_GAP;
        let next = open_sale(&rig, handle, now);
        assert_eq!(next.index, 1_000);
        assert_eq!(
            next.payment.address,
            wallet.receive_address(1_000).unwrap().to_string()
        );
    }
}
