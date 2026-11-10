//! Sign a PSBT file with the signer's rules, on regtest only.
//!
//! ```text
//! cargo run -p openbmt-signer --example regtest_sign -- \
//!     <state file> <descriptor> <master tprv> <psbt in> <psbt out> <unix time>
//! ```
//!
//! `scripts/signer-interop-core.sh` uses this to check the signer against
//! Bitcoin Core: Core builds the transaction file, this signs it as the Key
//! app would, and Core adds the other signature, finalizes and broadcasts.
//!
//! The state file holds the signer's state between runs. If it does not
//! exist, the wallet is registered from `descriptor` with empty state. The
//! account key is derived from the master key at `48'/1'/0'/2'`. The PSBT
//! files are raw bytes, not base64.
//!
//! The output is one line per fact (`fee`, `output`, `totals` and so on),
//! ending in `signed <txid>` (exit code 0) or `refused <rule>: <reason>`
//! (exit code 2). Any other problem is exit code 1.
//!
//! This is a test tool. It takes a private key on the command line, which a
//! real signer must never do, and it is built for regtest alone.

use std::fs;
use std::io::ErrorKind;
use std::process::ExitCode;
use std::str::FromStr;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use openbmt_signer::{OutputKind, Refusal, SignError, SignerState};

const NETWORK: Network = Network::Regtest;
const USAGE: &str =
    "usage: regtest_sign <state file> <descriptor> <master tprv> <psbt in> <psbt out> <unix time>";

fn refused(r: &Refusal) -> ExitCode {
    println!("refused {r}");
    ExitCode::from(2)
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [state_path, descriptor, master, psbt_in, psbt_out, now] = args.as_slice() else {
        return Err(USAGE.to_string());
    };
    let now: u64 = now.parse().map_err(|_| "the time is not a number")?;
    let secp = Secp256k1::new();
    let master = Xpriv::from_str(master).map_err(|e| format!("master key: {e}"))?;
    let path = DerivationPath::from_str("48'/1'/0'/2'").map_err(|e| e.to_string())?;
    let account = master
        .derive_priv(&secp, &path)
        .map_err(|e| format!("account key: {e}"))?;
    let account_public = Xpub::from_priv(&secp, &account);

    let mut state = match fs::read(state_path) {
        Ok(bytes) => SignerState::from_bytes(&bytes, NETWORK, &account_public)
            .map_err(|e| format!("state file: {e}"))?,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            SignerState::register(descriptor, NETWORK, &account_public, None)
                .map_err(|e| format!("register: {e}"))?
        }
        Err(e) => return Err(format!("state file: {e}")),
    };
    let given = SignerState::register(descriptor, NETWORK, &account_public, None)
        .map_err(|e| format!("descriptor: {e}"))?;
    if given.wallet() != state.wallet() {
        return Err("the state file is for another wallet".to_string());
    }

    let file = fs::read(psbt_in).map_err(|e| format!("{psbt_in}: {e}"))?;
    let review = match state.review(&file, now) {
        Ok(review) => review,
        Err(r) => return Ok(refused(&r)),
    };
    println!("txid {}", review.txid());
    println!(
        "inputs {} {}",
        review.input_count(),
        review.input_total_sats()
    );
    for o in review.outputs() {
        let kind = match &o.kind {
            OutputKind::Change { index } => format!("change {index}"),
            OutputKind::Allowlisted { label } => format!("allowlisted {label}"),
            OutputKind::Other => "other".to_string(),
        };
        let address = o
            .address
            .as_ref()
            .map_or_else(|| "-".to_string(), ToString::to_string);
        println!("output {} {} {address} {kind}", o.index, o.amount);
    }
    println!("fee {}", review.fee_sats());
    println!("vsize {}", review.vsize());
    println!(
        "already-counted {}",
        if review.already_counted() {
            "yes"
        } else {
            "no"
        }
    );

    // A real Key app stores the state so that a crash cannot leave half of
    // it behind. One write is enough for a drill.
    let signed = match state.sign(review, &account, now, |bytes| fs::write(state_path, bytes)) {
        Ok(signed) => signed,
        Err(SignError::Refused(r)) => return Ok(refused(&r)),
        Err(SignError::Persist(e)) => return Err(format!("could not store the state: {e}")),
    };
    fs::write(psbt_out, &signed.psbt).map_err(|e| format!("{psbt_out}: {e}"))?;
    let totals = state.totals(now);
    println!(
        "totals {} {} {} {}",
        totals.other_day, totals.other_week, totals.allowlisted_day, totals.allowlisted_week
    );
    println!("log-entries {}", state.log().count());
    println!("change-mark {}", state.change_mark());
    println!("signed {}", signed.txid);
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
