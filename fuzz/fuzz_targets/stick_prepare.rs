#![no_main]

use std::sync::OnceLock;

use bitcoin::bip32::Xpriv;
use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_stick::sign::prepare;

fn master() -> &'static Xpriv {
    static MASTER: OnceLock<Xpriv> = OnceLock::new();
    MASTER.get_or_init(|| Xpriv::new_master(Network::Bitcoin, &[7; 32]).unwrap())
}

// The Stick's PSBT intake with a fixed key: arbitrary bytes must never panic.
// This branch's `prepare` has no cap on the file and no check for bytes left
// after the PSBT, so nothing more is asserted of a file that prepares; add
// the assertions (under 8 MiB, no trailing bytes) where the check lands.
fuzz_target!(|data: &[u8]| {
    let _ = prepare(data, master());
});
