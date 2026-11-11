//! The counter pairing ceremony from end to end (DESIGN section 2.9,
//! `docs/COUNTER-RULES.md` CR-91 to CR-97): the Key app's offer, the
//! counter's request, a session on the real approver and owner crates with a
//! pairing, settings and a batch, the counter bundle, "They match", `finish`,
//! and a start from the stored record that opens a sale on the first batch.
//! Every call on the counter's side goes through `dispatch`.

use std::sync::OnceLock;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::{All, Secp256k1};
use bitcoin::Network;
use openbmt_approver::{Approver, Choice, Detail};
use openbmt_core::batch::{BatchBuilder, BatchPolicy, SignedBatch};
use openbmt_core::bundle::CounterBundle;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing::{PairOffer, PairRequest, SignedPairing};
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_owner::{Delivery, Options, Owner, OwnerError};
use openbmt_signer::SignerState;

use super::kit::{peers_event, Rig, FIVE_DOLLARS, RATE, T0};
use super::wire::{Rd, W};
use crate::StartConfig;

const NET: Network = Network::Regtest;
/// The network tag of regtest in the protocol.
const TAG_REGTEST: u8 = 3;
const OWNER_SECRET: [u8; 32] = [0x11; 32];
const APPROVER_SECRET: [u8; 32] = [0x22; 32];
const PEER: &str = "127.0.0.1:18444";

fn secp() -> &'static Secp256k1<All> {
    static CONTEXT: OnceLock<Secp256k1<All>> = OnceLock::new();
    CONTEXT.get_or_init(Secp256k1::new)
}

/// Three keys from fixed seeds, and the wallet they make. Test only.
fn keys() -> (&'static [Xpriv; 3], &'static WalletDescription) {
    static KIT: OnceLock<([Xpriv; 3], WalletDescription)> = OnceLock::new();
    let kit = KIT.get_or_init(|| {
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let mut accounts = Vec::new();
        let mut slots = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(NET, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(secp(), &path).unwrap();
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(secp()),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(secp(), &account),
            });
            accounts.push(account);
        }
        let wallet = WalletDescription::new(NET, slots.try_into().unwrap()).unwrap();
        (accounts.try_into().unwrap(), wallet)
    });
    (&kit.0, &kit.1)
}

fn xpub(n: usize) -> Xpub {
    Xpub::from_priv(secp(), &keys().0[n])
}

fn policy() -> BatchPolicy {
    BatchPolicy { min_count: 3 }
}

fn identity(secret: [u8; 32]) -> Identity {
    Identity::from_secret_bytes(&secret).unwrap()
}

/// The owner phone with key 1 and the Key app with key 2, as in the owner
/// crate's own tests.
struct Phones {
    owner: Owner,
    approver: Approver,
    key1: SignerState,
}

impl Phones {
    fn new() -> Self {
        let wallet = keys().1.clone();
        let owner_id = identity(OWNER_SECRET).device_id();
        let approver_id = identity(APPROVER_SECRET).device_id();
        let owner = Owner::new(
            &OWNER_SECRET,
            wallet.clone(),
            approver_id,
            policy(),
            Options::default(),
        )
        .unwrap();
        let mut approver =
            Approver::new(&APPROVER_SECRET, wallet.clone(), owner_id, policy()).unwrap();
        let key2 = SignerState::register(wallet.canonical_string(), NET, &xpub(1), Some(&xpub(2)))
            .unwrap();
        approver.attach_signer(key2, keys().0[1]).unwrap();
        let key1 = SignerState::register(wallet.canonical_string(), NET, &xpub(0), Some(&xpub(2)))
            .unwrap();
        Self {
            owner,
            approver,
            key1,
        }
    }

    /// What the Key app shows as the pairing offer.
    fn offer(&self, network: Network) -> Vec<u8> {
        PairOffer::new(
            self.owner.owner_id(),
            self.owner.approver_id(),
            keys().1.checksum32(),
            network,
        )
        .unwrap()
        .to_bytes()
    }

    /// Queue what the owner phone queues for a new counter, run the session
    /// through the approver and take the response. Returns the words the Key
    /// app showed for the pairing and the bundle the owner phone shows.
    fn session(
        &mut self,
        request: &[u8],
        settings_counters: &[DeviceId],
        with_batch: bool,
        at: u64,
    ) -> Result<(Vec<String>, Vec<u8>), String> {
        let counter = PairRequest::from_bytes(request).unwrap().counter;
        self.owner
            .queue_pairing(request, |_| Ok::<(), String>(()))
            .map_err(|e| e.to_string())?;
        let mut settings = Settings::defaults(1_000);
        settings.version = 1;
        for id in settings_counters {
            settings.devices.insert(*id);
            settings
                .counters
                .insert(*id, 10_000 / settings_counters.len() as u16);
        }
        self.owner
            .queue_settings(settings, |_| Ok::<(), String>(()))
            .map_err(|e| e.to_string())?;
        if with_batch {
            let batch = BatchBuilder::new(policy())
                .derive(keys().1, counter, 0, 3)
                .unwrap();
            self.owner
                .queue_batch(batch, at, |_| Ok::<(), String>(()))
                .map_err(|e| e.to_string())?;
        }
        let built = self
            .owner
            .build_request(&[], 1_000, 0, 850_000, at, &[7; 96], |_| {
                Ok::<(), String>(())
            })
            .map_err(|e: OwnerError<String>| e.to_string())?;
        let mut session = self
            .approver
            .open(&built.request, at)
            .map_err(|e| e.to_string())?;
        let mut words = Vec::new();
        let items: Vec<u32> = session.lines().iter().map(|l| l.item).collect();
        for item in items {
            let line = session
                .lines()
                .into_iter()
                .find(|l| l.item == item)
                .unwrap();
            if let Detail::Pairing(p) = &line.detail {
                words = p
                    .session_words
                    .expect("the words")
                    .iter()
                    .map(|w| (*w).to_string())
                    .collect();
            }
            assert!(line.can_approve, "{line:?}");
            session.decide(item, Choice::Approve).unwrap();
        }
        let response = session
            .finish(|_| Ok::<(), String>(()))
            .map_err(|e| e.to_string())?;
        self.owner
            .take_response(&response, &mut self.key1, &keys().0[0], at, |_, _| {
                Ok::<(), String>(())
            })
            .map_err(|e| e.to_string())?;
        // The owner phone's bundle: what was delivered, in one code.
        let (mut pairing, mut signed_settings, mut first) = (None, None, None);
        for d in self.owner.outbox() {
            match &d.what {
                Delivery::Pairing(b) => pairing = Some(SignedPairing::from_bytes(b).unwrap()),
                Delivery::Settings(b) => {
                    signed_settings = Some(SignedSettings::from_bytes(b).unwrap())
                }
                Delivery::Batch(b) => first = Some(SignedBatch::from_bytes(b).unwrap()),
                Delivery::Transaction { .. } => panic!("no transaction was asked for"),
            }
        }
        let bundle = CounterBundle::new(
            pairing.expect("a pairing"),
            signed_settings.expect("settings"),
            first,
            vec![PEER.to_string()],
        )
        .unwrap();
        Ok((words, bundle.to_bytes()))
    }
}

fn begin_fields(offer: &[u8], now: u64) -> Vec<u8> {
    W::new().bytes(offer).string("Front counter").u64(now).0
}

/// `pair_begin`: the request bytes and the words, or the refusal's rule.
fn begin(rig: &Rig, offer: &[u8], now: u64) -> Result<(Vec<u8>, Vec<String>), String> {
    let response = rig.call(44, 0, &begin_fields(offer, now));
    if let Some(e) = Rd::error(&response) {
        return Err(e.rule);
    }
    let mut r = Rd::ok(&response);
    let request = r.bytes();
    let words = r.list(|r| r.string());
    r.done();
    Ok((request, words))
}

fn matched(rig: &Rig, now: u64) -> Result<(), String> {
    let response = rig.call(45, 0, &W::new().u64(now).0);
    match Rd::error(&response) {
        Some(e) => Err(e.rule),
        None => {
            Rd::ok(&response).done();
            Ok(())
        }
    }
}

/// `pair_finish`: the stored record, or the refusal's rule.
fn finish(rig: &Rig, bundle: &[u8], now: u64) -> Result<Vec<u8>, String> {
    let response = rig.call(48, 0, &W::new().u64(now).bytes(bundle).0);
    if let Some(e) = Rd::error(&response) {
        assert!(!e.words.heading.is_empty(), "{e:?}");
        return Err(e.rule);
    }
    let mut r = Rd::ok(&response);
    let record = r.bytes();
    r.done();
    Ok(record)
}

fn counter_of(request: &[u8]) -> DeviceId {
    PairRequest::from_bytes(request).unwrap().counter
}

#[test]
fn cr_92_the_ceremony_runs_from_the_offer_to_a_node_that_opens_a_sale() {
    let mut phones = Phones::new();
    let rig = Rig::new();
    let offer = phones.offer(NET);
    let (request, words) = begin(&rig, &offer, T0).expect("begin");
    assert_eq!(words.len(), 4);
    let counter = counter_of(&request);

    // The Key app shows the same four words for the request it carries.
    let (key_words, bundle) = phones
        .session(&request, &[counter], true, T0 + 30)
        .expect("the session");
    assert_eq!(key_words, words);

    matched(&rig, T0 + 60).expect("they match");
    let record = finish(&rig, &bundle, T0 + 90).expect("finish");

    // The record tells the app the network and the settings before the start.
    let response = rig.call(43, 0, &W::new().bytes(&record).0);
    let mut r = Rd::ok(&response);
    assert_eq!(r.u8(), TAG_REGTEST);
    assert_eq!((r.u32(), r.bool()), (1, false));

    // The ceremony is over: a second finish has nothing to finish.
    assert_eq!(finish(&rig, &bundle, T0 + 91), Err("CR-91".to_string()));

    // The counter starts from the record alone and sells on the first batch.
    let handle = rig.start_with(&StartConfig {
        dir: rig.dir().to_str().unwrap().to_string(),
        now: T0 + 100,
        record,
        proxy: None,
        own_node: None,
    });
    rig.send(vec![peers_event()]);
    let response = rig.call(10, handle, &W::new().u64(T0 + 100).u32(50).0);
    Rd::ok(&response);
    let response = rig.call(
        11,
        handle,
        &W::new()
            .open(T0 + 100, FIVE_DOLLARS, "USD", RATE, "test-fixed")
            .0,
    );
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    assert_eq!(opened.index, 0);
    assert!(!opened.payment.address.is_empty());
}

#[test]
fn cr_92_a_bundle_is_not_taken_before_they_match() {
    let mut phones = Phones::new();
    let rig = Rig::new();
    let (request, _) = begin(&rig, &phones.offer(NET), T0).unwrap();
    let (_, bundle) = phones
        .session(&request, &[counter_of(&request)], true, T0 + 30)
        .unwrap();
    assert_eq!(finish(&rig, &bundle, T0 + 60), Err("CR-92".to_string()));
    // The refusal changed nothing: matching and finishing still work.
    matched(&rig, T0 + 70).unwrap();
    finish(&rig, &bundle, T0 + 80).expect("finish after they match");
}

#[test]
fn cr_92_a_bundle_for_another_counter_is_refused() {
    let rig = Rig::new();
    let mut phones = Phones::new();
    let offer = phones.offer(NET);
    let (request, _) = begin(&rig, &offer, T0).unwrap();
    // A second counter's ceremony, on its own phones.
    let other = Rig::new();
    let (other_request, _) = begin(&other, &offer, T0).unwrap();
    assert_ne!(counter_of(&request), counter_of(&other_request));
    let mut other_phones = Phones::new();
    let (_, bundle) = other_phones
        .session(&other_request, &[counter_of(&other_request)], true, T0 + 30)
        .unwrap();
    matched(&rig, T0 + 60).unwrap();
    assert_eq!(finish(&rig, &bundle, T0 + 90), Err("CR-92".to_string()));
    // The ceremony ended with the refusal.
    assert_eq!(finish(&rig, &bundle, T0 + 91), Err("CR-91".to_string()));
    let _ = &mut phones;
}

#[test]
fn cr_95_settings_that_do_not_list_this_counter_are_refused() {
    let mut phones = Phones::new();
    let rig = Rig::new();
    let (request, _) = begin(&rig, &phones.offer(NET), T0).unwrap();
    let stranger = identity([0x41; 32]).device_id();
    let (_, bundle) = phones
        .session(&request, &[stranger], false, T0 + 30)
        .unwrap();
    matched(&rig, T0 + 60).unwrap();
    assert_eq!(finish(&rig, &bundle, T0 + 90), Err("CR-95".to_string()));
}

#[test]
fn cr_96_the_first_batch_is_held_to_the_counters_own_minimum() {
    let mut phones = Phones::new();
    let rig = Rig::new();
    // The tests lower the minimum to 1; here the counter's own rule is back.
    *rig.registry().batch_min.lock().unwrap() = None;
    let (request, _) = begin(&rig, &phones.offer(NET), T0).unwrap();
    let (_, bundle) = phones
        .session(&request, &[counter_of(&request)], true, T0 + 30)
        .unwrap();
    matched(&rig, T0 + 60).unwrap();
    assert_eq!(finish(&rig, &bundle, T0 + 90), Err("CR-96".to_string()));
}

#[test]
fn cr_93_an_offer_for_another_network_does_not_get_past_the_owner_phone() {
    let mut phones = Phones::new();
    let rig = Rig::new();
    // The counter accepts what the Key app of another network showed...
    let (request, _) = begin(&rig, &phones.offer(Network::Bitcoin), T0).unwrap();
    // ...and the owner phone, on its own network, refuses the request.
    let refused = phones.session(&request, &[counter_of(&request)], true, T0 + 30);
    assert!(refused.is_err(), "{refused:?}");
    assert!(phones.owner.pairings().is_empty());
}

#[test]
fn cr_91_a_ceremony_whose_window_has_passed_is_refused() {
    let mut phones = Phones::new();
    let rig = Rig::new();
    let (request, _) = begin(&rig, &phones.offer(NET), T0).unwrap();
    let (_, bundle) = phones
        .session(&request, &[counter_of(&request)], true, T0 + 30)
        .unwrap();
    matched(&rig, T0 + 60).unwrap();
    let late = T0 + 60 * 60;
    let refused = finish(&rig, &bundle, late).unwrap_err();
    assert!(refused.starts_with("CR-9"), "{refused}");
    // Nothing was kept.
    assert_eq!(finish(&rig, &bundle, T0 + 90), Err("CR-91".to_string()));
}

#[test]
fn cr_92_they_differ_ends_the_ceremony_with_words() {
    let mut phones = Phones::new();
    let rig = Rig::new();
    let (request, _) = begin(&rig, &phones.offer(NET), T0).unwrap();
    let (_, bundle) = phones
        .session(&request, &[counter_of(&request)], true, T0 + 30)
        .unwrap();
    let response = rig.call(46, 0, &[]);
    let mut r = Rd::ok(&response);
    let words = r.words();
    r.done();
    assert!(words.next.contains("Key app"), "{words:?}");
    assert_eq!(matched(&rig, T0 + 60), Err("CR-91".to_string()));
    assert_eq!(finish(&rig, &bundle, T0 + 90), Err("CR-91".to_string()));
}
