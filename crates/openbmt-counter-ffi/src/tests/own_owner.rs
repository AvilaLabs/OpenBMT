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
use openbmt_owner_node::{
    own_approver, OwnNode, OwnerConfig, OwnerLink, OwnerLinkConfig, OwnerNode, OwnerParts,
    OwnerPort,
};

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

impl OwnerPort for IdlePort {
    fn broadcast(&self, _: Vec<u8>) {}
    fn forget(&self, _: bitcoin::Txid) {}
    fn unwatch_scripts(&self, _: Vec<bitcoin::ScriptBuf>) {}
    fn rescan_from(&self, _: u32) {}
    fn set_fetch_transactions(&self, _: bool) {}
    fn fetch_block(&self, _: bitcoin::BlockHash) {}
}

/// The owner phone as the app has it: the node, the folder, the relay
/// hint and the owner's end of the link once a counter is paired.
struct Owner {
    node: OwnNode,
    wallet: PinnedWallet,
    dir: Scratch,
    link_dir: Scratch,
    listener: Option<TcpListener>,
    server: Option<Server>,
    link: Option<OwnerLink<RelayPost>>,
    /// The owner node the app runs (OA-107): the live watch over the book.
    runner: Option<OwnerNode<IdlePort>>,
    carried: u32,
    waiting_given: Option<[u8; 32]>,
    _events: Arc<std::sync::Mutex<Vec<std::sync::mpsc::Sender<AppEvent>>>>,
}

impl Owner {
    fn new(text: &str) -> Self {
        let dir = Scratch::new();
        let node = OwnNode::open(dir.path(), id(1), PinnedWallet::from_text(text).unwrap())
            .expect("the owner node opens");
        Self {
            node,
            wallet: PinnedWallet::from_text(text).unwrap(),
            dir,
            link_dir: Scratch::new(),
            listener: Some(TcpListener::bind("127.0.0.1:0").unwrap()),
            server: None,
            link: None,
            runner: None,
            carried: 0,
            waiting_given: None,
            _events: Arc::default(),
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
        // The owner node of OA-107, as the app starts it: on the book.
        let secret = [1u8; 32];
        let mut parts = OwnerParts::new(
            secret,
            self.wallet.wallet().clone(),
            own_approver(&secret).unwrap(),
            openbmt_core::batch::BatchPolicy { min_count: 3 },
        )
        .unwrap();
        parts.own_book = Some(self.node.shop().book().clone());
        let senders = Arc::clone(&self._events);
        self.runner = Some(
            OwnerNode::start_with(
                OwnerConfig::new(
                    self.dir.path(),
                    id(1),
                    DriverConfig::new(Network::Bitcoin, 0),
                ),
                parts,
                move |_, _, _| {
                    let (tx, rx) = channel();
                    senders.lock().unwrap().push(tx);
                    Ok(IdlePort(rx.into()))
                },
            )
            .unwrap(),
        );
    }

    /// One turn of the owner's side as the app makes it: the link's step,
    /// then the shop's tick on the shop as the file holds it.
    fn step(&mut self, now: u64) -> openbmt_owner_node::OwnTick {
        let runner = self.runner.as_mut().unwrap();
        let link = self.link.as_mut().unwrap();
        link.step(runner.watcher_mut(), now);
        let mut shop = OwnNode::open(self.dir.path(), id(1), self.wallet.clone()).unwrap();
        let tick = shop
            .tick(
                runner,
                link,
                &mut self.carried,
                &mut self.waiting_given,
                now,
            )
            .unwrap();
        // LK-79: the owner node's turn on the starter line.
        runner.starter_line_step(link, now);
        tick
    }
}

/// A turn of both ends of the link: the counter's step, then the owner's.
fn turn(rig: &Rig, handle: u64, owner: &mut Owner, now: u64) -> openbmt_owner_node::OwnTick {
    let response = rig.call(OP_LINK_STEP, handle, &W::new().u64(now).u8(0).0);
    let mut r = Rd::ok(&response);
    let _ = r.link();
    let _ = r.list(Rd::string);
    r.done();
    owner.step(now)
}

/// Turns until the owner's copy of the counter's chain stops growing.
fn settle(rig: &Rig, handle: u64, owner: &mut Owner, mut now: u64) -> u64 {
    let counter = owner.link.as_ref().unwrap().counters()[0];
    let mut last = 0;
    let mut quiet = 0;
    for _ in 0..40 {
        now += 10;
        let _ = turn(rig, handle, owner, now);
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

        // OA-107: the owner node's live watch follows every index of the
        // range, and the gap past it.
        assert_eq!(
            owner.runner.as_ref().unwrap().view().live_count(),
            1_000 + 20 + 20
        );

        // 720 sales in all leave 280 of the 1,000 indexes unused. The owner
        // reads the counter's records and, in its own turn, gives the next
        // range without being asked (OA-112).
        let mut now = T0 + 100;
        for _ in 1..720 {
            now += SALE_GAP;
            let opened = open_sale(&rig, handle, now);
            assert!(opened.index < 1_000);
        }
        let counter = read.counter;
        let mut topped = Vec::new();
        for _ in 0..40 {
            now += 10;
            let turn = turn(&rig, handle, &mut owner, now);
            topped.extend(turn.topped_up);
            if !topped.is_empty() {
                break;
            }
        }
        assert_eq!(topped.len(), 1, "the node sent the next range by itself");
        assert_eq!((topped[0].start, topped[0].end), (1_000, 2_000));
        assert_eq!(topped[0].counter, counter);
        assert!(owner
            .link
            .as_ref()
            .unwrap()
            .ranges_waiting(&counter)
            .is_some());
        assert_eq!(
            owner.runner.as_ref().unwrap().view().live_count(),
            2_000 + 20 + 20,
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
        let shop = OwnNode::open(owner.dir.path(), id(1), owner.wallet.clone()).unwrap();
        assert_eq!(shop.unused_of(link, &counter), Some(1_280));
        // The outbox is empty, and no further range goes out.
        assert!(owner.step(now + 1).topped_up.is_empty());

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

const OP_PENDING_SWITCH: u16 = 200;
const OP_SWITCH_SAME: u16 = 201;
const OP_SWITCH_RECORD_KEPT: u16 = 202;

#[test]
fn ss_41_the_real_owner_switches_the_real_counter_from_the_starter_wallet_and_ss_42_may_then_move()
{
    use openbmt_owner::own_switch::SwitchStatus;

    let old = PinnedWallet::from_text(SINGLE).unwrap();
    let new = PinnedWallet::from_text(MULTI).unwrap();
    let mut owner = Owner::new(SINGLE);

    // CR-256: the offer of a starter wallet is marked, and the counter that
    // takes it is one that may switch.
    let made = owner.node.shop().make_offer_as(T0, true).unwrap();
    let rig = Rig::new();
    begin(&rig, &made.offer.to_bytes()).expect("the counter reads it");
    let (request, _) = same(&rig, T0 + 10).expect("same address");
    let read = owner
        .node
        .shop()
        .read_request(&made, T0 + 20, &request)
        .unwrap();
    matched(&rig, T0 + 30).expect("they match on the counter");
    let paired = owner.confirm(&made, T0 + 30, &request);
    let record = finish(&rig, &paired.bundle.to_bytes(), T0 + 40).expect("the counter pairs");
    {
        let (shop, store) = owner.node.shop_and_store();
        shop.mark_delivered(&read.counter, &store).unwrap();
    }
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
    Rd::ok(&rig.call(OP_PUMP, handle, &W::new().u64(T0 + 100).u32(50).0));
    let first = open_sale(&rig, handle, T0 + 100);
    assert_eq!(
        first.payment.address,
        old.receive_address(first.index).unwrap().to_string()
    );

    // SS-41: before the switch, the counter shows as waiting and SS-42's
    // move is not ready.
    let mut now = settle(&rig, handle, &mut owner, T0 + 100);
    {
        let shop = OwnNode::open(owner.dir.path(), id(1), owner.wallet.clone()).unwrap();
        let link = owner.link.as_ref().unwrap();
        assert_eq!(
            shop.switch_rows(link, &new),
            vec![(read.counter, SwitchStatus::Waiting)]
        );
        assert!(shop.move_ready(link, &new).is_err());
    }

    // CR-257: the owner makes the switch and sends it on the link.
    {
        let mut shop = OwnNode::open(owner.dir.path(), id(1), owner.wallet.clone()).unwrap();
        let link = owner.link.as_mut().unwrap();
        let made = shop.send_switches(link, &new, now + 1).unwrap();
        assert_eq!(made, vec![read.counter]);
    }
    // The counter takes it on its next step and shows the new wallet's
    // first address (CR-260).
    let mut pending = None;
    for _ in 0..40 {
        now += 10;
        let _ = turn(&rig, handle, &mut owner, now);
        let response = rig.call(OP_PENDING_SWITCH, handle, &W::new().u64(now).0);
        let mut r = Rd::ok(&response);
        pending = r.opt(|r| (r.string(), r.u8(), r.u64(), r.u64()));
        if pending.is_some() {
            break;
        }
    }
    let (address, kind, _, _) = pending.expect("the counter holds the switch");
    assert_eq!(address, new.receive_address(0).unwrap().to_string());
    assert_eq!(kind, 2, "two of three");

    // CR-260, CR-261: the manager says "Same address".
    Rd::ok(&rig.call(OP_SWITCH_SAME, handle, &W::new().u64(now + 1).0));
    // The app keeps the new pairing record, then says so (CR-261).
    Rd::ok(&rig.call(OP_SWITCH_RECORD_KEPT, handle, &W::new().u64(now + 2).0));
    for _ in 0..40 {
        now += 10;
        let _ = turn(&rig, handle, &mut owner, now);
        if owner.link.as_ref().unwrap().switches_waiting().is_empty() {
            break;
        }
    }

    // CR-262: the owner reads the counter's record: switched, nothing waits,
    // and the move is ready (SS-42).
    {
        let shop = OwnNode::open(owner.dir.path(), id(1), owner.wallet.clone()).unwrap();
        let link = owner.link.as_ref().unwrap();
        assert!(link.switches_waiting().is_empty());
        assert_eq!(
            shop.switch_rows(link, &new),
            vec![(read.counter, SwitchStatus::Switched)]
        );
        assert!(shop.move_ready(link, &new).is_ok());
    }

    // The next sale pays the new wallet, at an index above every old one.
    let next = open_sale(&rig, handle, now + 100);
    assert!(next.index >= 1_000, "{}", next.index);
    assert_eq!(
        next.payment.address,
        new.receive_address(next.index).unwrap().to_string()
    );
}

const OP_STARTER_STATE: u16 = 204;

#[test]
fn lk_79_the_real_owner_node_sends_the_real_counter_its_starter_line_through_the_relay() {
    let mut owner = Owner::new(SINGLE);
    let made = owner.node.shop().make_offer_as(T0, true).unwrap();
    let rig = Rig::new();
    begin(&rig, &made.offer.to_bytes()).expect("the counter reads it");
    let (request, _) = same(&rig, T0 + 10).expect("same address");
    let read = owner
        .node
        .shop()
        .read_request(&made, T0 + 20, &request)
        .unwrap();
    matched(&rig, T0 + 30).expect("they match on the counter");
    let paired = owner.confirm(&made, T0 + 30, &request);
    let record = finish(&rig, &paired.bundle.to_bytes(), T0 + 40).expect("the counter pairs");
    {
        let (shop, store) = owner.node.shop_and_store();
        shop.mark_delivered(&read.counter, &store).unwrap();
    }
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
    Rd::ok(&rig.call(OP_PUMP, handle, &W::new().u64(T0 + 100).u32(50).0));
    let state = |now: u64| {
        let response = rig.call(OP_STARTER_STATE, handle, &W::new().u64(now).0);
        let mut r = Rd::ok(&response);
        let decision = r.u8();
        let _words = r.opt(|r| r.words().heading);
        let _cards = r.opt(|r| r.words().heading);
        let held = r.opt(Rd::u64);
        let limit = r.opt(Rd::u64);
        let _time = r.opt(Rd::u64);
        let payments = r.list(|r| (r.bytes(), r.u64(), r.u64(), r.bool(), r.bool()));
        r.done();
        (decision, held, limit, payments)
    };
    // Until the app gives the limit, the owner node makes no line and the
    // counter offers bitcoin as before.
    let mut now = settle(&rig, handle, &mut owner, T0 + 100);
    assert_eq!(state(now), (0, None, None, Vec::new()));
    let runner = owner.runner.as_mut().unwrap();
    assert_eq!(
        runner.starter_held(),
        Some(0),
        "the new wallet holds nothing"
    );

    // The app gives the limit and the stop setting: the next turns carry a
    // line through the relay and the counter takes it (CR-272). The wallet
    // holds nothing, which is under any limit, so bitcoin is still offered.
    runner.set_starter_line(100_000, true);
    for _ in 0..40 {
        now += 10;
        let _ = turn(&rig, handle, &mut owner, now);
        if state(now).1.is_some() {
            break;
        }
    }
    assert_eq!(state(now), (0, Some(0), Some(100_000), Vec::new()));
    assert!(rig.dir().join("starter-line.bin").exists());
    // The app lowers the limit to nothing: the wallet holds as much as the
    // limit, and the counter offers no bitcoin (CR-273) once the new line
    // arrives.
    owner.runner.as_mut().unwrap().set_starter_line(0, true);
    for _ in 0..40 {
        now += 10;
        let _ = turn(&rig, handle, &mut owner, now);
        if state(now).0 == 1 {
            break;
        }
    }
    assert_eq!(state(now), (1, Some(0), Some(0), Vec::new()));
    // The line ages: with no turn of the owner's side the limit cannot be
    // checked, and bitcoin is offered again.
    assert_eq!(state(now + 1_000).0, 2);
}
