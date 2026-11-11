#![no_main]

use std::sync::OnceLock;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, WPubkeyHash,
    Witness,
};
use libfuzzer_sys::fuzz_target;
use openbmt_approver::{Approver, Choice};
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_owner::{Options, Owner, SaleFacts, SaleKey};
use openbmt_signer::SignerState;
use openbmt_spend::Coin;

const NETWORK: Network = Network::Regtest;
const NOW: u64 = 10_000_000;
const OWNER_SECRET: [u8; 32] = [0x11; 32];
const APPROVER_SECRET: [u8; 32] = [0x22; 32];

/// An owner with one open session (a refund asked of the Key app), key 1's
/// signer, and the Key app's valid response, so that mutated bytes get past
/// the frame and the binding to the request.
struct Fixture {
    wallet: WalletDescription,
    approver: DeviceId,
    account: Xpriv,
    key1: SignerState,
    owner_snapshot: Vec<u8>,
    response: Vec<u8>,
}

fn policy() -> BatchPolicy {
    BatchPolicy { min_count: 3 }
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let secp = Secp256k1::new();
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let mut accounts = Vec::new();
        let mut slots = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(NETWORK, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(&secp, &path).unwrap();
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(&secp),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(&secp, &account),
            });
            accounts.push(account);
        }
        let xpubs: Vec<Xpub> = accounts.iter().map(|a| Xpub::from_priv(&secp, a)).collect();
        let wallet = WalletDescription::new(NETWORK, slots.try_into().unwrap()).unwrap();
        let owner_id = Identity::from_secret_bytes(&OWNER_SECRET).unwrap().device_id();
        let approver = Identity::from_secret_bytes(&APPROVER_SECRET).unwrap().device_id();
        let register = |xpub: &Xpub| {
            SignerState::register(wallet.canonical_string(), NETWORK, xpub, Some(&xpubs[2]))
                .unwrap()
        };
        let key1 = register(&xpubs[0]);

        // The owner asks for a refund of a sale it knows is paid and deep.
        let mut owner = Owner::new(
            &OWNER_SECRET,
            wallet.clone(),
            approver,
            policy(),
            Options::default(),
        )
        .unwrap();
        let sale = SaleKey {
            counter: Identity::from_secret_bytes(&[0x31; 32]).unwrap().device_id(),
            index: 1,
        };
        let ok = |_: &[u8]| Ok::<(), String>(());
        owner
            .set_sale_facts(
                sale,
                SaleFacts {
                    received_sats: 50_000,
                    reached_refund_depth: true,
                },
                ok,
            )
            .unwrap();
        owner
            .queue_refund(
                sale,
                "Mia",
                ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([1; 20])),
                20_000,
                None,
                ok,
            )
            .unwrap();
        let coin = coin(&wallet, 1, 0, 200_000);
        let mut random = [7u8; 96];
        for b in random.iter_mut().skip(32) {
            *b = 107;
        }
        let built = owner
            .build_request(&[coin], 1_000, 0, 850_000, NOW, &random, ok)
            .unwrap();

        // The Key app approves every line it can.
        let mut key_app = Approver::new(&APPROVER_SECRET, wallet.clone(), owner_id, policy())
            .unwrap();
        key_app
            .attach_signer(register(&xpubs[1]), accounts[1])
            .unwrap();
        let mut session = key_app.open(&built.request, NOW).unwrap();
        let items: Vec<u32> = session.lines().iter().map(|l| l.item).collect();
        for item in items {
            let line = session
                .lines()
                .into_iter()
                .find(|l| l.item == item)
                .unwrap();
            let choice = if line.can_approve {
                Choice::Approve
            } else {
                Choice::Decline("cannot".to_string())
            };
            session.decide(item, choice).unwrap();
        }
        let response = session.finish(ok).unwrap();
        Fixture {
            wallet,
            approver,
            account: accounts[0],
            key1,
            owner_snapshot: owner.snapshot().unwrap(),
            response,
        }
    })
}

/// A coin of `sats` on the receive keychain at `index`.
fn coin(wallet: &WalletDescription, tag: u32, index: u32, sats: u64) -> Coin {
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
                script_pubkey: ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([250; 20])),
            },
            TxOut {
                value: Amount::from_sat(sats),
                script_pubkey: wallet.receive_script(index).unwrap(),
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

fuzz_target!(|data: &[u8]| {
    let f = fixture();
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    // The first byte picks the bytes handed in: the input as it is, the
    // valid response with the input written over its tail from a place the
    // byte gives, or the valid response with one bit changed.
    let response: Vec<u8> = match mode % 3 {
        0 => rest.to_vec(),
        1 => {
            let keep = usize::from(mode >> 2).min(f.response.len());
            let mut v = f.response[..keep].to_vec();
            v.extend_from_slice(rest);
            v
        }
        _ => {
            let mut v = f.response.clone();
            if let Some(&at) = rest.first() {
                let at = (usize::from(at) | usize::from(*rest.get(1).unwrap_or(&0)) << 8) % v.len();
                v[at] ^= 1 << (mode >> 2 & 7);
            }
            v
        }
    };
    let Ok(mut owner) = Owner::restore(
        &OWNER_SECRET,
        f.wallet.clone(),
        f.approver,
        policy(),
        &f.owner_snapshot,
    ) else {
        panic!("the fixed snapshot restores");
    };
    let mut key1 = f.key1.clone();
    let before = owner.snapshot().unwrap();
    let taken = owner.take_response(&response, &mut key1, &f.account, NOW, |_, _| {
        Ok::<(), String>(())
    });
    if taken.is_err() {
        // A refused response changes nothing.
        assert_eq!(owner.snapshot().unwrap(), before);
    }
});
