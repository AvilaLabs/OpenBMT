//! The shop the demo owner starts from.

use super::*;
use openbmt_core::pinned_wallet::PinnedWallet;

const T0: u64 = 1_700_000_000;

#[test]
fn the_wallet_file_is_the_demo_wallet_the_counter_takes_payments_into() {
    let file = wallet_file().expect("a file");
    let read = PinnedWallet::from_bytes(&file).expect("it reads back");
    assert_eq!(read, demo_wallet().expect("the wallet"));
    assert_eq!(
        address(0).expect("an address"),
        "bcrt1q9p0hpmws4pupj85nrw7d8uxna83wuyp0wnmg50"
    );
}

#[test]
fn the_seeded_state_gives_the_demo_counter_its_first_range_and_is_kept_when_seeded_again() {
    let dir = std::env::temp_dir().join(format!("openbmt-demo-owner-seed-{}", std::process::id()));
    let dir = dir.to_str().expect("UTF-8");
    seed(dir, T0).expect("seeded");
    let bytes = fs::read(format!("{dir}/{OWN_STATE_FILE}")).expect("the state is there");
    let shop = OwnShop::restore(
        owner().expect("owner"),
        demo_wallet().expect("wallet"),
        &bytes,
    )
    .expect("it reads back");
    let counter = counter().expect("counter").device_id();
    let ranges = shop.book().ranges_of(&counter);
    assert_eq!(ranges.len(), 1);
    assert_eq!((ranges[0].start, ranges[0].end), (0, 1_000));
    // A second seed at a later time changes nothing.
    seed(dir, T0 + 60).expect("seeded again");
    assert_eq!(
        fs::read(format!("{dir}/{OWN_STATE_FILE}")).expect("state"),
        bytes
    );
}
