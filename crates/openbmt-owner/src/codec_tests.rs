//! Tests of what the queue's snapshot refuses to write (AS-65, AS-88).

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::Identity;
use openbmt_core::wallet::{KeySlot, WalletDescription};

use super::*;
use crate::types::{Options, Refund, SaleKey, Stage};
use bitcoin::hashes::Hash;

const NET: Network = Network::Regtest;

fn owner() -> Owner {
    let secp = Secp256k1::new();
    let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
    let slots: Vec<KeySlot> = (0..3u8)
        .map(|n| {
            let master = Xpriv::new_master(NET, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(&secp, &path).unwrap();
            KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(&secp),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(&secp, &account),
            }
        })
        .collect();
    let wallet = WalletDescription::new(NET, slots.try_into().unwrap()).unwrap();
    let approver = Identity::from_secret_bytes(&[0x22; 32])
        .unwrap()
        .device_id();
    Owner::new(
        &[0x11; 32],
        wallet,
        approver,
        BatchPolicy { min_count: 3 },
        Options::default(),
    )
    .unwrap()
}

/// A refund that waits in a session, as taking one of two payments of a
/// session once left it: the session is not open.
fn in_a_session_that_is_gone(owner: &Owner) -> State {
    let mut st = owner.state.clone();
    let counter = Identity::from_secret_bytes(&[0x31; 32])
        .unwrap()
        .device_id();
    let id = st.take_id().unwrap();
    st.refunds.push(Refund {
        id,
        sale: SaleKey { counter, index: 1 },
        cashier: "Mia".to_owned(),
        script: bitcoin::ScriptBuf::new_p2wpkh(&bitcoin::WPubkeyHash::from_byte_array([9; 20])),
        sats: 40_000,
        shop_value: None,
        books: None,
        conflict: Vec::new(),
        stage: Stage::InSession { session: [7; 32] },
    });
    st
}

#[test]
fn as_88_a_queue_that_would_not_restore_is_not_written() {
    let owner = owner();
    assert!(encode(&owner, &owner.state).is_ok());
    let broken = in_a_session_that_is_gone(&owner);
    assert!(matches!(
        encode(&owner, &broken),
        Err(Refusal::QueueNotSaved(_))
    ));
}

#[test]
fn as_88_a_call_that_would_store_such_a_queue_stores_nothing() {
    let mut owner = owner();
    owner.state = in_a_session_that_is_gone(&owner);
    let mut stored = 0;
    let refused = owner.set_options(Options::default(), |_| {
        stored += 1;
        Ok::<(), String>(())
    });
    assert!(matches!(
        refused,
        Err(crate::error::OwnerError::Refused(Refusal::QueueNotSaved(_)))
    ));
    assert_eq!(stored, 0);
}
