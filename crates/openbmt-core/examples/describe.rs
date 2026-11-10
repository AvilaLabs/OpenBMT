//! Print a wallet description's canonical form and some of its addresses.
//!
//! ```text
//! cargo run -p openbmt-core --example describe -- <network> <descriptor> [index ...]
//! ```
//!
//! `network` is one of `bitcoin`, `testnet`, `signet`, `regtest`. The output
//! is one line `descriptor <canonical string>`, the two single-path forms as
//! `receive-descriptor <string>` and `change-descriptor <string>`, then for
//! each index a line `receive <index> <address>` and a line
//! `change <index> <address>`.
//! `scripts/restore-drill-core.sh` compares these lines with what Bitcoin
//! Core derives from the same descriptor.

use std::process::ExitCode;

use bitcoin::Network;
use openbmt_core::wallet::{Keychain, WalletDescription};

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: describe <network> <descriptor> [index ...]";
    let network = match args.next().ok_or(usage)?.as_str() {
        "bitcoin" => Network::Bitcoin,
        "testnet" => Network::Testnet,
        "signet" => Network::Signet,
        "regtest" => Network::Regtest,
        other => return Err(format!("unknown network: {other}")),
    };
    let text = args.next().ok_or(usage)?;
    let wallet = WalletDescription::parse(&text, network).map_err(|e| e.to_string())?;
    println!("descriptor {}", wallet.canonical_string());
    let [receive, change] = wallet.single_path_strings();
    println!("receive-descriptor {receive}");
    println!("change-descriptor {change}");
    for arg in args {
        let index: u32 = arg.parse().map_err(|_| format!("bad index: {arg}"))?;
        for (name, chain) in [("receive", Keychain::Receive), ("change", Keychain::Change)] {
            let address = wallet.address(chain, index).map_err(|e| e.to_string())?;
            println!("{name} {index} {address}");
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
