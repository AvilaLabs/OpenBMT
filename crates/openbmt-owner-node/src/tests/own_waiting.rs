//! The owner node's waiting change (`docs/OWNER-APP-RULES.md` OA-119 to
//! OA-121): its file is written before anything is sent, read strictly at
//! the start, and deleted by a start that finds it in force.

use openbmt_core::approval::CounterConfirmation;
use openbmt_core::pairing_own::OwnCounterPairing;
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_owner::own_shop::MadeOffer;

use super::*;
use crate::own::{OwnNode, OwnNodeError, OWN_WAITING_FILE};

const SINGLE: &str = "wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx";
const USD: u64 = 100_000;

fn open(dir: &Dir) -> Result<OwnNode, OwnNodeError> {
    OwnNode::open(&dir.0, id(1), PinnedWallet::from_text(SINGLE).unwrap())
}

fn request(made: &MadeOffer, n: u8) -> Vec<u8> {
    OwnCounterPairing::begin(
        made.offer.clone(),
        &id(n),
        "Front",
        [n; 32],
        made.made_at + 5,
    )
    .unwrap()
    .1
    .to_bytes()
}

/// A node whose shop has counter 3 and Box 9, and whose second counter waits.
fn node_with_a_waiting_change(dir: &Dir) -> OwnNode {
    let mut node = open(dir).unwrap();
    let writer = node.waiting_writer();
    let (shop, store) = node.shop_and_store();
    let made = shop.make_offer(T0).unwrap();
    shop.confirm_with(
        &made,
        T0 + 30,
        &request(&made, 3),
        USD,
        vec![],
        None,
        &store,
        &writer,
    )
    .unwrap();
    shop.add_box(id(9).device_id(), 0, &store).unwrap();
    let made = shop.make_offer(T0 + 100).unwrap();
    let paired = shop
        .confirm_with(
            &made,
            T0 + 130,
            &request(&made, 4),
            USD,
            vec![],
            None,
            &store,
            &writer,
        )
        .unwrap();
    assert!(paired.waiting);
    node
}

#[test]
fn oa_119_the_file_is_written_before_the_pairing_is_handed_over_and_read_back_at_the_start() {
    let dir = Dir::new();
    let node = node_with_a_waiting_change(&dir);
    assert!(dir.0.join(OWN_WAITING_FILE).exists());
    let shown = node.waiting_view().unwrap();
    drop(node);
    let again = open(&dir).unwrap();
    assert_eq!(again.waiting_view(), Some(shown.clone()));
    assert_eq!(shown.version, 3);
    assert_eq!(shown.fields, vec!["devices", "counters"]);
    assert_eq!(shown.ends_at, shown.signed_at + 7 * 24 * 60 * 60);
}

#[test]
fn oa_121_a_file_that_does_not_read_back_stops_the_start() {
    let dir = Dir::new();
    drop(node_with_a_waiting_change(&dir));
    let path = dir.0.join(OWN_WAITING_FILE);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.push(0);
    std::fs::write(&path, &bytes).unwrap();
    assert!(matches!(open(&dir), Err(OwnNodeError::State(_))));
    std::fs::write(&path, &bytes[..bytes.len() - 5]).unwrap();
    assert!(matches!(open(&dir), Err(OwnNodeError::State(_))));
}

#[test]
fn oa_120_a_start_that_finds_the_settings_in_force_deletes_the_file() {
    let dir = Dir::new();
    let mut node = node_with_a_waiting_change(&dir);
    let kept = std::fs::read(dir.0.join(OWN_WAITING_FILE)).unwrap();
    let hash = openbmt_owner::own_waiting::WaitingChange::from_bytes(&kept)
        .unwrap()
        .hash();
    let confirmed = node
        .shop()
        .join_confirmation(CounterConfirmation::sign(&id(3), &hash))
        .unwrap();
    let (shop, store) = node.shop_and_store();
    shop.put_in_force(&confirmed, store).unwrap();
    drop(node);
    // The crash came after the settings were in force and before the file was deleted.
    std::fs::write(dir.0.join(OWN_WAITING_FILE), &kept).unwrap();
    let again = open(&dir).unwrap();
    assert!(again.waiting_view().is_none());
    assert!(!dir.0.join(OWN_WAITING_FILE).exists());
}
