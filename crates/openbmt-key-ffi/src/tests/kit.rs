//! The test kit: an in-memory `KeyStore`, fixed keys for the other devices
//! and the owner phone's side of a session, built with the owner crate.
//!
//! The keys come from fixed seeds: test only, never use. The owner crate
//! is used in two places only, `build_request` and `queue_all`, so a change
//! to its calls is a change in one place.

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, WPubkeyHash,
    Witness,
};
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing::CounterPairing;
use openbmt_core::settings::Settings;
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_owner::{Built, Options, Owner, SaleFacts, SaleKey};
use openbmt_signer::SignerState;
use openbmt_spend::Coin;

use crate::{
    EntropySource, Key2Public, KeyApp, KeyNetwork, KeyStore, SessionHandle, SetupState, StoreError,
    WalletSetup, WalletView,
};

pub const NET: Network = Network::Regtest;
pub const NOW: u64 = 10_000_000;
pub const OWNER_SECRET: [u8; 32] = [0x11; 32];
pub const SMALL_BATCH: u32 = 3;
/// The PIN of every Key app the tests make (SR-72).
pub const PIN: &str = "482913";

/// A Key app on a store with nothing in it, with its PIN set.
pub fn open_new(store: Arc<MemStore>) -> Arc<KeyApp> {
    let app = KeyApp::open(store).expect("opens empty");
    app.set_pin(PIN.to_string()).expect("a PIN is set");
    app
}

/// A Key app on a store that holds a state, unlocked with the PIN.
pub fn reopen(store: Arc<MemStore>) -> Arc<KeyApp> {
    let app = KeyApp::open(store).expect("opens");
    app.unlock(PIN.to_string()).expect("unlocks");
    app
}

// ---- the app's side: an in-memory store ---------------------------------------

#[derive(Default)]
struct MemState {
    saved: Option<Vec<u8>>,
    saves: usize,
    fail_save: bool,
    now: u64,
    drawn: u64,
}

/// What the Android app implements, in memory. It can be made to fail and it
/// counts its saves. Its random bytes are a hash stream: test only.
pub struct MemStore(Mutex<MemState>);

impl MemStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(MemState {
            now: NOW,
            ..MemState::default()
        })))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, MemState> {
        self.0.lock().expect("the test store is not poisoned")
    }

    pub fn saved(&self) -> Option<Vec<u8>> {
        self.state().saved.clone()
    }

    /// The bytes inside the locked frame that is stored: the `OBM7`
    /// envelope, opened with the tests' PIN.
    pub fn plain(&self) -> Option<Vec<u8>> {
        self.saved().map(|b| {
            crate::lock::open(&b, PIN)
                .map_err(|_| "the stored frame opens with the tests' PIN")
                .expect("opens")
                .0
        })
    }

    pub fn saves(&self) -> usize {
        self.state().saves
    }

    pub fn fail_saves(&self, yes: bool) {
        self.state().fail_save = yes;
    }

    pub fn set_now(&self, now: u64) {
        self.state().now = now;
    }

    pub fn set_saved(&self, bytes: Option<Vec<u8>>) {
        self.state().saved = bytes;
    }
}

impl KeyStore for MemStore {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.state().saved.clone())
    }

    fn save(&self, bytes: Vec<u8>) -> Result<(), StoreError> {
        let mut s = self.state();
        if s.fail_save {
            return Err(StoreError::Failed {
                text: "disk full".to_string(),
            });
        }
        s.saves += 1;
        s.saved = Some(bytes);
        Ok(())
    }

    fn random(&self, n: u32) -> Result<Vec<u8>, StoreError> {
        let mut s = self.state();
        let mut out = Vec::new();
        while out.len() < usize::try_from(n).expect("n fits") {
            s.drawn += 1;
            let block =
                sha256::Hash::hash(&[b"key-ffi-test".as_slice(), &s.drawn.to_le_bytes()].concat());
            out.extend_from_slice(block.as_byte_array());
        }
        out.truncate(usize::try_from(n).expect("n fits"));
        Ok(out)
    }

    fn now(&self) -> u64 {
        self.state().now
    }
}

// ---- the other devices ---------------------------------------------------------

pub fn secp() -> Secp256k1<bitcoin::secp256k1::All> {
    Secp256k1::new()
}

pub fn path() -> DerivationPath {
    "m/48'/1'/0'/2'".parse().expect("a path")
}

/// An account key and its slot, from a fixed seed.
pub fn fixed_key(seed: u8, index: u8) -> (Xpriv, KeySlot) {
    let master = Xpriv::new_master(NET, &[seed; 32]).expect("a master key");
    let account = master
        .derive_priv(&secp(), &path())
        .expect("an account key");
    let slot = KeySlot {
        index,
        origin_fingerprint: master.fingerprint(&secp()),
        origin_path: path(),
        xpub: Xpub::from_priv(&secp(), &account),
    };
    (account, slot)
}

pub fn identity(secret: [u8; 32]) -> Identity {
    Identity::from_secret_bytes(&secret).expect("a valid secret")
}

pub fn owner_id() -> DeviceId {
    identity(OWNER_SECRET).device_id()
}

pub fn counter(n: u8) -> Identity {
    identity([0x30 + n; 32])
}

pub fn policy() -> BatchPolicy {
    BatchPolicy {
        min_count: SMALL_BATCH,
    }
}

/// Fixed rolls for a key made from dice, 120 of them.
pub fn rolls() -> String {
    "123456".repeat(20)
}

// ---- a Key app, set up -----------------------------------------------------------

/// The Key app, the owner phone's side and key 1, with the wallet accepted.
pub struct Rig {
    pub app: Arc<KeyApp>,
    pub store: Arc<MemStore>,
    pub owner: Owner,
    pub wallet: WalletDescription,
    pub key1: SignerState,
    pub key1_account: Xpriv,
    pub view: WalletView,
}

/// A wallet description from the Key app's key 2 and the fixed keys 1 and 3.
pub fn wallet_with(app: &KeyApp) -> (WalletDescription, Xpriv, Xpub) {
    let public = app
        .key2_public()
        .expect("asked")
        .expect("this device holds key 2");
    wallet_from_public(&public)
}

/// The same, from the public parts of key 2 as the app gave them.
pub fn wallet_from_public(public: &Key2Public) -> (WalletDescription, Xpriv, Xpub) {
    let (key1, slot1) = fixed_key(1, 1);
    let (_, slot3) = fixed_key(3, 3);
    let slot2 = KeySlot {
        index: 2,
        origin_fingerprint: Fingerprint::from_str(&public.master_fingerprint).expect("hex"),
        origin_path: path(),
        xpub: public.account_xpub.parse().expect("an xpub"),
    };
    let key3 = slot3.xpub;
    (
        WalletDescription::new(NET, [slot1, slot2, slot3]).expect("a wallet"),
        key1,
        key3,
    )
}

/// The key text a signer or another wallet shows for `slot`.
pub fn origin_text_of(slot: &KeySlot) -> String {
    openbmt_signer::KeyPart {
        fingerprint: slot.origin_fingerprint,
        xpub: slot.xpub,
    }
    .origin_text()
}

/// The `OBMw` bytes the owner phone sends for `wallet`.
pub fn setup_bytes_for(wallet: &WalletDescription) -> Vec<u8> {
    openbmt_signer::WalletSetupBytes {
        network: NET,
        description: wallet.clone(),
        owner_root: owner_id(),
        batch_min_count: SMALL_BATCH,
    }
    .to_bytes()
}

/// A Key app with no key 2 of its own, given the wallet by the ceremony: key
/// 3 and key 2 taken from their texts, the setup accepted and confirmed.
pub fn accept_with_no_key_2(app: &crate::KeyApp, wallet: &WalletDescription) {
    let slot = |i: usize| wallet.slots().get(i).expect("a slot").clone();
    app.take_key3(origin_text_of(&slot(2))).expect("key 3");
    app.take_key2(origin_text_of(&slot(1))).expect("key 2");
    app.accept_wallet_setup(setup_bytes_for(wallet))
        .expect("accepted");
    app.confirm_wallet().expect("confirmed");
}

pub fn setup_for(wallet: &WalletDescription, key3: &Xpub) -> WalletSetup {
    WalletSetup {
        description: wallet.canonical_string().to_string(),
        network: KeyNetwork::Regtest,
        owner_root: owner_id().to_bytes().to_vec(),
        key3_xpub: Some(key3.to_string()),
        batch_min_count: Some(SMALL_BATCH),
    }
}

/// The Key app made from dice and given the wallet: the whole of setup.
pub fn rig() -> Rig {
    let store = MemStore::new();
    let app = open_new(store.clone());
    assert_eq!(app.state().expect("state"), SetupState::Empty);
    app.create_from_entropy(EntropySource::Platform, KeyNetwork::Regtest)
        .expect("a key is made");
    let (wallet, key1_account, key3) = wallet_with(&app);
    let view = app
        .accept_wallet(setup_for(&wallet, &key3))
        .expect("the wallet is accepted");
    let approver = DeviceId::from_bytes(
        &app.device_root()
            .expect("the root")
            .try_into()
            .expect("32 bytes"),
    )
    .expect("a root");
    let owner = Owner::new(
        &OWNER_SECRET,
        wallet.clone(),
        approver,
        policy(),
        Options::default(),
    )
    .expect("an owner");
    let key1 = SignerState::register(
        wallet.canonical_string(),
        NET,
        &Xpub::from_priv(&secp(), &key1_account),
        Some(&key3),
    )
    .expect("key 1's signer");
    Rig {
        app,
        store,
        owner,
        wallet,
        key1,
        key1_account,
        view,
    }
}

// ---- the owner phone's side -------------------------------------------------------

/// The address of `external(n)`, in the tests' network.
pub fn address(n: u8) -> String {
    bitcoin::Address::from_script(&external(n), NET)
        .expect("an address")
        .to_string()
}

pub fn external(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([n; 20]))
}

/// A coin of `sats` on the receive keychain at `index`, 6 deep.
pub fn coin(wallet: &WalletDescription, tag: u32, index: u32, sats: u64) -> Coin {
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
                script_pubkey: wallet.receive_script(index).expect("a script"),
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

pub fn ok_store(_: &[u8]) -> Result<(), String> {
    Ok(())
}

/// The one place that calls the owner crate's `build_request`.
pub fn build_request(owner: &mut Owner, coins: &[Coin], change_index: u32, tag: u8) -> Built {
    let mut random = [tag; 96];
    random[32..].fill(tag.wrapping_add(100));
    owner
        .build_request(coins, 1_000, change_index, 0, NOW, &random, ok_store)
        .expect("the request is built")
}

/// Queue a pairing (from the Key app's own offer), settings that add the
/// counter, and one refund of `sats` to `external(1)`.
pub fn queue_all(rig: &mut Rig, refund_sats: u64) -> CounterPairing {
    let offer = rig.app.pair_offer_bytes().expect("the offer");
    let (pairing, request) =
        CounterPairing::begin(&offer, &counter(1), "Counter 1", [1; 32], NOW).expect("a request");
    rig.owner
        .queue_pairing(&request.to_bytes(), ok_store)
        .expect("the pairing is queued");
    let mut settings = Settings::defaults(1_000);
    settings.version = 1;
    let id = counter(1).device_id();
    settings.devices.insert(id);
    settings.counters.insert(id, 10_000);
    rig.owner
        .queue_settings(settings, ok_store)
        .expect("the settings are queued");
    let sale = SaleKey {
        counter: id,
        index: 1,
    };
    rig.owner
        .set_sale_facts(
            sale,
            SaleFacts {
                received_sats: 50_000,
                reached_refund_depth: true,
            },
            ok_store,
        )
        .expect("facts");
    rig.owner
        .queue_refund(sale, "Mia", external(1), refund_sats, None, ok_store)
        .expect("the refund is queued");
    pairing
}

/// A request with a pairing, settings, one refund and the health check.
pub fn session_request(rig: &mut Rig) -> Vec<u8> {
    queue_all(rig, 10_000);
    let coins: Vec<Coin> = (0..2)
        .map(|i| coin(&rig.wallet, i + 1, i, 300_000))
        .collect();
    build_request(&mut rig.owner, &coins, 0, 1).request
}

/// Open a session, approve every line, finish.
pub fn approve_all(rig: &Rig, request: &[u8]) -> (std::sync::Arc<SessionHandle>, Vec<u8>) {
    let session = rig.app.open_session(request.to_vec()).expect("opens");
    for line in session.lines().expect("lines") {
        session
            .decide(line.item, crate::Decision::Approve)
            .expect("decides");
    }
    let response = session.finish().expect("finishes");
    (session, response)
}
