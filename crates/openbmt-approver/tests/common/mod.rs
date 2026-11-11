//! Fixed keys, coins, requests and stores for the tests.
//!
//! The keys come from fixed seeds: test only, never use.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::sync::OnceLock;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::{All, Secp256k1};
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, WPubkeyHash,
    Witness,
};
use openbmt_approver::Approver;
use openbmt_core::batch::{AddressBatch, BatchBuilder, BatchPolicy, PinnedRoots};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing::{CounterPairing, PairOffer};
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_session::{Item, Label, OutputLabel, Request, RequestItem};
use openbmt_signer::{Ceilings, SignerState};
use openbmt_spend::Coin;

pub const NET: Network = Network::Regtest;
pub const NOW: u64 = 10_000_000;
pub const SMALL_BATCH: u32 = 3;

pub fn secp() -> &'static Secp256k1<All> {
    static CONTEXT: OnceLock<Secp256k1<All>> = OnceLock::new();
    CONTEXT.get_or_init(Secp256k1::new)
}

pub struct Kit {
    pub accounts: [Xpriv; 3],
    pub xpubs: [Xpub; 3],
    pub wallet: WalletDescription,
}

/// Three keys from fixed seeds, and the wallet they make. Test only.
pub fn kit() -> &'static Kit {
    static KIT: OnceLock<Kit> = OnceLock::new();
    KIT.get_or_init(|| {
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
        let xpubs = [0, 1, 2].map(|n| Xpub::from_priv(secp(), &accounts[n]));
        Kit {
            accounts: accounts.try_into().unwrap(),
            xpubs,
            wallet,
        }
    })
}

pub fn wallet() -> &'static WalletDescription {
    &kit().wallet
}

pub fn wallet_id() -> [u8; 32] {
    wallet().checksum32()
}

// ---- identities ----------------------------------------------------------

pub const OWNER_SECRET: [u8; 32] = [0x11; 32];
pub const APPROVER_SECRET: [u8; 32] = [0x22; 32];

pub fn identity(secret: [u8; 32]) -> Identity {
    Identity::from_secret_bytes(&secret).unwrap()
}

pub fn owner() -> Identity {
    identity(OWNER_SECRET)
}

pub fn approver_id() -> DeviceId {
    identity(APPROVER_SECRET).device_id()
}

pub fn counter(n: u8) -> Identity {
    identity([0x30 + n; 32])
}

pub fn policy() -> BatchPolicy {
    BatchPolicy {
        min_count: SMALL_BATCH,
    }
}

pub fn roots() -> PinnedRoots {
    PinnedRoots::new(owner().device_id(), approver_id()).unwrap()
}

// ---- approvers -------------------------------------------------------------

/// A signer for key 2 (the wallet's second slot), with `other` ceilings for
/// other destinations when given.
pub fn signer_state(other: Option<Ceilings>) -> SignerState {
    let k = kit();
    let mut s = SignerState::register(
        wallet().canonical_string(),
        NET,
        &k.xpubs[1],
        Some(&k.xpubs[2]),
    )
    .unwrap();
    if let Some(c) = other {
        s.set_ceilings(c, openbmt_signer::FIRST_ALLOWLISTED, |_| {
            Ok::<(), Infallible>(())
        })
        .unwrap();
    }
    s
}

/// A Key app that holds key 2.
pub fn approver_with_key2() -> Approver {
    approver_with_key2_ceilings(None)
}

pub fn approver_with_key2_ceilings(other: Option<Ceilings>) -> Approver {
    let mut a = approver_only();
    a.attach_signer(signer_state(other), kit().accounts[1])
        .unwrap();
    a
}

/// A Key app that is an approver only.
pub fn approver_only() -> Approver {
    Approver::new(
        &APPROVER_SECRET,
        wallet().clone(),
        owner().device_id(),
        policy(),
    )
    .unwrap()
}

/// Restore the Key app from stored bytes.
pub fn restore(bytes: &[u8], with_key2: bool) -> Result<Approver, openbmt_approver::RestoreError> {
    Approver::restore(
        &APPROVER_SECRET,
        wallet().clone(),
        owner().device_id(),
        policy(),
        with_key2.then(|| kit().accounts[1]),
        bytes,
    )
}

/// A store that keeps the last snapshot and can be made to fail.
#[derive(Default)]
pub struct Store {
    pub last: RefCell<Option<Vec<u8>>>,
    pub calls: RefCell<usize>,
    pub fail: RefCell<bool>,
}

impl Store {
    pub fn put(&self, bytes: &[u8]) -> Result<(), String> {
        *self.calls.borrow_mut() += 1;
        if *self.fail.borrow() {
            return Err("disk full".to_string());
        }
        *self.last.borrow_mut() = Some(bytes.to_vec());
        Ok(())
    }
}

// ---- coins and spends -----------------------------------------------------

pub fn external(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([n; 20]))
}

/// A coin of `sats` on the receive keychain at `index`, 6 deep. `tag` makes
/// the outpoint unique.
pub fn coin(tag: u32, index: u32, sats: u64) -> Coin {
    let mut funding = [0u8; 32];
    funding[..4].copy_from_slice(&tag.to_le_bytes());
    let prev = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array(funding),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![
            TxOut {
                value: Amount::from_sat(1),
                script_pubkey: external(250),
            },
            TxOut {
                value: Amount::from_sat(sats),
                script_pubkey: wallet().receive_script(index).unwrap(),
            },
        ],
    };
    Coin {
        outpoint: OutPoint {
            txid: prev.compute_txid(),
            vout: 1,
        },
        prev_tx: prev,
        vout: 1,
        chain: openbmt_core::wallet::Keychain::Receive,
        index,
        confirmations: 6,
    }
}

pub fn spend_request(
    payments: Vec<(ScriptBuf, u64)>,
    change_index: u32,
) -> openbmt_spend::SpendRequest {
    openbmt_spend::SpendRequest {
        payments,
        fee_rate: 1_000,
        change_index,
        min_confirmations: 1,
        send_all: false,
        must_spend: Vec::new(),
        lock_height: 0,
        change_position: openbmt_spend::ChangePosition::Last,
    }
}

/// The PSBT bytes of a plan that pays `payments` from `coins`.
pub fn psbt_bytes(coins: &[Coin], payments: Vec<(ScriptBuf, u64)>, change_index: u32) -> Vec<u8> {
    let plan =
        openbmt_spend::plan(wallet(), coins, &spend_request(payments, change_index)).unwrap();
    plan.to_psbt(wallet()).unwrap().serialize()
}

/// The PSBT bytes of a plan with the change at `position` among the outputs
/// (AS-24: the line comes from the signer's review, whatever the place).
pub fn psbt_bytes_with_change_at(
    coins: &[Coin],
    payments: Vec<(ScriptBuf, u64)>,
    change_index: u32,
    position: usize,
) -> Vec<u8> {
    // A seed that puts the change at `position` is found by search; the place
    // depends on the seed and the inputs only.
    for n in 0..=u8::MAX {
        let mut request = spend_request(payments.clone(), change_index);
        request.lock_height = 850_000;
        request.change_position = openbmt_spend::ChangePosition::Seed([n; 32]);
        let plan = openbmt_spend::plan(wallet(), coins, &request).unwrap();
        if plan.change_position() == Some(position) {
            return plan.to_psbt(wallet()).unwrap().serialize();
        }
    }
    panic!("no seed puts the change at {position}");
}

pub fn refund_label(output: u32, name: &str) -> OutputLabel {
    OutputLabel {
        output,
        label: Label::Refund {
            counter: counter(1).device_id(),
            sale_index: output,
            cashier: name.to_string(),
            shop_value: None,
        },
    }
}

pub fn change_label(output: u32) -> OutputLabel {
    OutputLabel {
        output,
        label: Label::Change,
    }
}

/// A spend item paying one refund of `sats` to `external(n)` from `coins`,
/// with honest labels.
pub fn refund_item(coins: &[Coin], n: u8, sats: u64, change_index: u32) -> Item {
    Item::Spend {
        psbt: psbt_bytes(coins, vec![(external(n), sats)], change_index),
        labels: vec![refund_label(0, "Mia"), change_label(1)],
    }
}

// ---- batches, settings, pairings -------------------------------------------

pub fn batch(counter_n: u8, start: u32, count: u32) -> AddressBatch {
    BatchBuilder::new(policy())
        .derive(wallet(), counter(counter_n).device_id(), start, count)
        .unwrap()
}

pub fn batch_item(counter_n: u8, start: u32, count: u32) -> Item {
    let b = batch(counter_n, start, count);
    let owner_sig = b.sign(&owner());
    Item::BatchRefill {
        batch: b,
        owner_sig,
    }
}

pub fn settings_with(version: u32, counters: &[u8]) -> Settings {
    let mut s = Settings::defaults(1_000);
    s.version = version;
    for c in counters {
        let id = counter(*c).device_id();
        s.devices.insert(id);
        s.counters.insert(id, 10_000 / counters.len() as u16);
    }
    s
}

pub fn settings_item(s: Settings) -> Item {
    let owner_sig = s.sign(&owner());
    Item::Settings {
        signed: SignedSettings {
            settings: s,
            owner_sig,
            approver_sig: None,
        },
    }
}

pub fn offer() -> PairOffer {
    PairOffer::new(owner().device_id(), approver_id(), wallet_id(), NET).unwrap()
}

pub fn pairing_item(counter_n: u8, claimed_version: u32) -> (Item, CounterPairing) {
    let (pairing, request) = CounterPairing::begin(
        &offer().to_bytes(),
        &counter(counter_n),
        "Counter 1",
        [counter_n; 32],
        NOW,
    )
    .unwrap();
    (
        Item::Pairing {
            request,
            settings_version: claimed_version,
        },
        pairing,
    )
}

pub fn health_item(n: u8) -> Item {
    Item::HealthCheck { nonce: [n; 32] }
}

// ---- requests --------------------------------------------------------------

pub fn numbered(items: Vec<Item>) -> Vec<RequestItem> {
    items
        .into_iter()
        .enumerate()
        .map(|(n, item)| RequestItem {
            number: n as u32,
            item,
        })
        .collect()
}

pub fn request_with(session: u8, items: Vec<Item>) -> Request {
    Request::new(&owner(), [session; 32], wallet_id(), numbered(items)).unwrap()
}

pub fn request_bytes(session: u8, items: Vec<Item>) -> Vec<u8> {
    request_with(session, items).as_bytes().to_vec()
}

pub fn counters_of(s: &Settings) -> BTreeSet<DeviceId> {
    s.counter_set()
}

pub fn shares(ids: &[u8]) -> BTreeMap<DeviceId, u16> {
    ids.iter().map(|c| (counter(*c).device_id(), 1)).collect()
}

// ---- running a session -------------------------------------------------------

pub fn refused(a: &mut Approver, bytes: &[u8]) -> openbmt_approver::RefuseSession {
    a.open(bytes, NOW).err().expect("the request is refused")
}

/// Open `bytes`, approve every line, finish into `store`. Returns the
/// response bytes.
pub fn approve_all(a: &mut Approver, bytes: &[u8], store: &Store) -> Vec<u8> {
    let mut session = a.open(bytes, NOW).expect("the request opens");
    let items: Vec<u32> = session.lines().iter().map(|l| l.item).collect();
    for item in items {
        session
            .decide(item, openbmt_approver::Choice::Approve)
            .expect("the line can be approved");
    }
    session
        .finish(|b| store.put(b))
        .expect("the session finishes")
}

/// Co-sign settings with `counters` on the device list, in a session of
/// their own.
pub fn cosign_settings(
    a: &mut Approver,
    session: u8,
    version: u32,
    counters: &[u8],
    store: &Store,
) {
    let bytes = request_bytes(
        session,
        vec![settings_item(settings_with(version, counters))],
    );
    approve_all(a, &bytes, store);
}

pub fn decode(response: &[u8], request: &Request) -> openbmt_session::Response {
    let r = openbmt_session::Response::from_bytes(response).unwrap();
    r.check_against(request, &approver_id()).unwrap();
    r
}
