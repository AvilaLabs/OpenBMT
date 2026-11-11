//! Fixed keys, coins, stores and a rig with the real crates on both sides.
//!
//! The keys come from fixed seeds: test only, never use.

#![allow(dead_code)]

use std::cell::RefCell;
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
use openbmt_approver::{Approver, Choice};
use openbmt_core::batch::{AddressBatch, BatchBuilder, BatchPolicy};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::Settings;
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_owner::{Built, Options, Owner, OwnerError, QueuedPairing, SaleFacts, SaleKey, Taken};
use openbmt_signer::SignerState;
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

// ---- identities -------------------------------------------------------------------

pub const OWNER_SECRET: [u8; 32] = [0x11; 32];
pub const APPROVER_SECRET: [u8; 32] = [0x22; 32];

pub fn identity(secret: [u8; 32]) -> Identity {
    Identity::from_secret_bytes(&secret).unwrap()
}

pub fn owner_id() -> DeviceId {
    identity(OWNER_SECRET).device_id()
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

// ---- stores -----------------------------------------------------------------------

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

    pub fn calls(&self) -> usize {
        *self.calls.borrow()
    }

    pub fn last(&self) -> Vec<u8> {
        self.last.borrow().clone().expect("something was stored")
    }

    pub fn fail_next(&self, yes: bool) {
        *self.fail.borrow_mut() = yes;
    }
}

// ---- owners and keys --------------------------------------------------------------

pub fn new_owner(options: Options) -> Owner {
    Owner::new(
        &OWNER_SECRET,
        wallet().clone(),
        approver_id(),
        policy(),
        options,
    )
    .unwrap()
}

pub fn restore_owner(bytes: &[u8]) -> Result<Owner, openbmt_owner::RestoreError> {
    Owner::restore(
        &OWNER_SECRET,
        wallet().clone(),
        approver_id(),
        policy(),
        bytes,
    )
}

/// Key 1's signer, as it is on the owner phone.
pub fn key1_signer() -> SignerState {
    let k = kit();
    SignerState::register(
        wallet().canonical_string(),
        NET,
        &k.xpubs[0],
        Some(&k.xpubs[2]),
    )
    .unwrap()
}

pub fn key1_account() -> Xpriv {
    kit().accounts[0]
}

/// Key 2's signer, as it is on the Key app.
pub fn key2_signer() -> SignerState {
    let k = kit();
    SignerState::register(
        wallet().canonical_string(),
        NET,
        &k.xpubs[1],
        Some(&k.xpubs[2]),
    )
    .unwrap()
}

pub fn new_approver(with_key2: bool) -> Approver {
    let mut a = Approver::new(&APPROVER_SECRET, wallet().clone(), owner_id(), policy()).unwrap();
    if with_key2 {
        a.attach_signer(key2_signer(), kit().accounts[1]).unwrap();
    }
    a
}

// ---- coins, sales, scripts --------------------------------------------------------

/// The outputs of `tx` that are not change to one of the wallet's first
/// sixteen change scripts, in the order of the transaction. The change can
/// stand at any place (AS-61).
pub fn pays(tx: &Transaction) -> Vec<&bitcoin::TxOut> {
    let change: Vec<ScriptBuf> = (0..16)
        .map(|i| wallet().change_script(i).unwrap())
        .collect();
    tx.output
        .iter()
        .filter(|o| !change.contains(&o.script_pubkey))
        .collect()
}

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
        chain: Keychain::Receive,
        index,
        confirmations: 6,
    }
}

pub fn sale(n: u32) -> SaleKey {
    SaleKey {
        counter: counter(1).device_id(),
        index: n,
    }
}

pub fn deep(received: u64) -> SaleFacts {
    SaleFacts {
        received_sats: received,
        reached_refund_depth: true,
    }
}

pub fn shallow(received: u64) -> SaleFacts {
    SaleFacts {
        received_sats: received,
        reached_refund_depth: false,
    }
}

pub fn set_facts(o: &mut Owner, s: SaleKey, f: SaleFacts) {
    o.set_sale_facts(s, f, |_| Ok::<(), String>(())).unwrap();
}

/// Facts for sale `n` (received `received`, at depth) and a refund of `sats`
/// to `external(n as u8)`.
pub fn refund(o: &mut Owner, n: u32, received: u64, sats: u64) -> u32 {
    set_facts(o, sale(n), deep(received));
    o.queue_refund(
        sale(n),
        "Mia",
        external(u8::try_from(n).unwrap()),
        sats,
        None,
        |_| Ok::<(), String>(()),
    )
    .unwrap()
}

// ---- batches, settings, pairings -------------------------------------------------

pub fn batch(counter_n: u8, start: u32, count: u32) -> AddressBatch {
    BatchBuilder::new(policy())
        .derive(wallet(), counter(counter_n).device_id(), start, count)
        .unwrap()
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

pub fn pair_request(
    owner: &Owner,
    counter_n: u8,
) -> (openbmt_core::pairing::CounterPairing, Vec<u8>) {
    let offer = openbmt_core::pairing::PairOffer::new(
        owner.owner_id(),
        owner.approver_id(),
        wallet_id(),
        NET,
    )
    .unwrap();
    let (pairing, request) = openbmt_core::pairing::CounterPairing::begin(
        &offer.to_bytes(),
        &counter(counter_n),
        "Counter 1",
        [counter_n; 32],
        NOW,
    )
    .unwrap();
    (pairing, request.to_bytes())
}

pub fn ok<T>(r: Result<T, OwnerError<String>>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    }
}

pub fn refused<T: std::fmt::Debug>(r: Result<T, OwnerError<String>>) -> openbmt_owner::Refusal {
    match r {
        Err(OwnerError::Refused(x)) => x,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

pub fn nop(_: &[u8]) -> Result<(), String> {
    Ok(())
}

// ---- the rig -----------------------------------------------------------------------

/// The owner phone, key 1, and the Key app with key 2, each with a store.
pub struct Rig {
    pub owner: Owner,
    pub approver: Approver,
    pub key1: SignerState,
    pub owner_store: Store,
    pub key1_store: Store,
    pub approver_store: Store,
    /// The lock height every transaction of the next sessions is built with.
    pub lock_height: u32,
    session_no: u8,
}

impl Rig {
    pub fn new() -> Self {
        Self::with(Options::default(), true)
    }

    pub fn with(options: Options, key2: bool) -> Self {
        Self {
            owner: new_owner(options),
            approver: new_approver(key2),
            key1: key1_signer(),
            owner_store: Store::default(),
            key1_store: Store::default(),
            approver_store: Store::default(),
            lock_height: 850_000,
            session_no: 0,
        }
    }

    /// Random bytes for the next session.
    pub fn random(&mut self) -> [u8; 96] {
        self.session_no += 1;
        let mut r = [self.session_no; 96];
        for b in r.iter_mut().skip(32) {
            *b = self.session_no.wrapping_add(100);
        }
        for b in r.iter_mut().skip(64) {
            *b = self.session_no.wrapping_mul(7).wrapping_add(200);
        }
        r
    }

    pub fn build(&mut self, coins: &[Coin], change_index: u32) -> Built {
        let random = self.random();
        let store = &self.owner_store;
        ok(self.owner.build_request(
            coins,
            1_000,
            change_index,
            self.lock_height,
            NOW,
            &random,
            |b| store.put(b),
        ))
    }

    /// The Key app opens the request and decides each line with `decide`.
    pub fn answer(
        &mut self,
        request: &[u8],
        decide: impl Fn(&openbmt_approver::Line) -> Choice,
    ) -> Vec<u8> {
        let mut session = self.approver.open(request, NOW).expect("the request opens");
        let items: Vec<u32> = session.lines().iter().map(|l| l.item).collect();
        for item in items {
            // A line sees what the lines before it did (AS-32), so the lines
            // are listed again before each decision.
            let line = session
                .lines()
                .into_iter()
                .find(|l| l.item == item)
                .expect("the line exists");
            let choice = match decide(&line) {
                Choice::Approve if !line.can_approve => Choice::Decline("cannot".to_string()),
                c => c,
            };
            session.decide(item, choice).unwrap();
        }
        let store = &self.approver_store;
        session
            .finish(|b| store.put(b))
            .expect("the session finishes")
    }

    pub fn answer_all(&mut self, request: &[u8]) -> Vec<u8> {
        self.answer(request, |_| Choice::Approve)
    }

    pub fn take(&mut self, response: &[u8]) -> Result<Taken, OwnerError<String>> {
        let (os, ks) = (&self.owner_store, &self.key1_store);
        self.owner
            .take_response(response, &mut self.key1, &key1_account(), NOW, |o, k| {
                os.put(o)?;
                ks.put(k)
            })
    }

    /// Build, approve every line, take.
    pub fn round(&mut self, coins: &[Coin], change_index: u32) -> Taken {
        let built = self.build(coins, change_index);
        let response = self.answer_all(&built.request);
        ok(self.take(&response))
    }
}

pub fn queued_pairing_of(owner: &Owner, id: u32) -> &QueuedPairing {
    owner.pairings().iter().find(|p| p.id == id).unwrap()
}
