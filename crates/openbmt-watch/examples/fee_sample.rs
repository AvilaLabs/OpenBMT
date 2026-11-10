//! Print the fee sample of one block, read as hex from standard input.
//!
//! ```text
//! cargo run -p openbmt-watch --example fee_sample -- <network> <height>
//! ```
//!
//! `network` is one of `bitcoin`, `testnet`, `testnet4`, `signet`,
//! `regtest`. The output is one line:
//! `height <n> block <hash> fees <sats> vsize <n> claimed_less <0|1>
//! rate_milli <n|none>`.
//! `scripts/fee-sample-core.sh` compares these numbers with what Bitcoin
//! Core reports for the same block.

use std::io::Read;
use std::process::ExitCode;

use bitcoin::consensus::encode::deserialize_hex;
use bitcoin::{Block, Network};
use openbmt_watch::block_fee_sample;

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: fee_sample <network> <height> < block-hex";
    let network = match args.next().ok_or(usage)?.as_str() {
        "bitcoin" => Network::Bitcoin,
        "testnet" => Network::Testnet,
        "testnet4" => Network::Testnet4,
        "signet" => Network::Signet,
        "regtest" => Network::Regtest,
        other => return Err(format!("unknown network: {other}")),
    };
    let height: u32 = args
        .next()
        .ok_or(usage)?
        .parse()
        .map_err(|_| "bad height".to_string())?;
    let mut text = String::new();
    std::io::stdin()
        .take(16_000_000)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let block: Block = deserialize_hex(text.trim()).map_err(|e| e.to_string())?;
    let s = block_fee_sample(&block, height, network).map_err(|e| e.to_string())?;
    let rate = s
        .rate_milli
        .map_or_else(|| "none".to_string(), |r| r.to_string());
    println!(
        "height {} block {} fees {} vsize {} claimed_less {} rate_milli {}",
        s.height,
        s.block,
        s.fees_sats,
        s.vsize,
        u8::from(s.claimed_less),
        rate
    );
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
