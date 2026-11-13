//! The refund float on regtest, driven by `scripts/float-e2e-core.sh`
//! (`docs/FLOAT-RULES.md`, FL-20 to FL-35).
//!
//! TEST ONLY, NEVER USE: the float's words are the published BIP-85 test
//! vector and the shop's key comes from fixed bytes, so anybody can spend
//! these wallets. Regtest only.
//!
//! Bitcoin Core plays the chain and the shop's wallet; the float is the real
//! library. The float's key is the same on both sides, under different
//! prefixes: the library holds the mainnet description of the account key
//! and Core's wallet holds the same account key private under the test
//! network's prefix; the scripts do not depend on the network.
//!
//! ```text
//! float keys --who float|shop
//! float pay --coins FILE --addr A --sats N --rate N --change-index N
//!     --lock-height N --out FILE
//! float sweep --coins FILE --shop-index N --rate N --lock-height N --out FILE
//! float refill --cap N --confirmed N --unconfirmed N [--amount N] --index N
//! ```
//!
//! The output is one fact per line. Exit code 0 is success, 2 a refusal by
//! the library, 1 any other problem.

use std::collections::HashMap;
use std::fs;
use std::process::ExitCode;
use std::str::FromStr;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::consensus::encode::{deserialize_hex, serialize_hex};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Address, Network, NetworkKind, OutPoint, Transaction, Txid};
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_owner::float::{
    pay_refund, refill_amount, shop_receive_script, sweep, Coin, FloatPaid, FloatRefusal,
    FloatWallet, FloatWords,
};

const NETWORK: Network = Network::Regtest;
const WORDS: &str = "girl mad pet galaxy egg matter matrix prison refuse sense ordinary nose";

struct Args(HashMap<String, String>);

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut map = HashMap::new();
    let mut it = args.iter();
    while let Some(name) = it.next() {
        let name = name
            .strip_prefix("--")
            .ok_or_else(|| format!("not an option: {name}"))?;
        let value = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
        map.insert(name.to_string(), value.clone());
    }
    Ok(Args(map))
}

impl Args {
    fn one(&self, name: &str) -> Result<&str, String> {
        self.0
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("--{name} is needed"))
    }
    fn number(&self, name: &str) -> Result<u64, String> {
        self.one(name)?
            .parse()
            .map_err(|_| format!("--{name} is a number"))
    }
    fn small(&self, name: &str) -> Result<u32, String> {
        u32::try_from(self.number(name)?).map_err(|_| format!("--{name} is too large"))
    }
}

fn read_coins(path: &str) -> Result<Vec<Coin>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut coins = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        let [txid, vout, conf, chain, index, hex] = f.as_slice() else {
            return Err(format!("{path}:{}: six fields are needed", n + 1));
        };
        let prev_tx: Transaction =
            deserialize_hex(hex).map_err(|e| format!("{path}:{}: {e}", n + 1))?;
        let vout: u32 = vout.parse().map_err(|_| "vout")?;
        coins.push(Coin {
            outpoint: OutPoint {
                txid: Txid::from_str(txid).map_err(|e| e.to_string())?,
                vout,
            },
            prev_tx,
            vout,
            chain: match *chain {
                "0" => Keychain::Receive,
                "1" => Keychain::Change,
                other => return Err(format!("{path}:{}: chain {other}", n + 1)),
            },
            index: index.parse().map_err(|_| "index")?,
            confirmations: conf.parse().map_err(|_| "confirmations")?,
        });
    }
    Ok(coins)
}

fn refused(e: &FloatRefusal) -> Result<ExitCode, String> {
    println!("refused {} {e}", e.rule());
    Ok(ExitCode::from(2))
}

/// The shop's wallet, from fixed bytes, as the owner phone holds it: the
/// public description of one account, and the private key for Core.
fn shop(secp: &Secp256k1<bitcoin::secp256k1::All>) -> Result<(SingleKeyWallet, Xpriv), String> {
    let master = Xpriv::new_master(Network::Bitcoin, &[0x77; 32]).map_err(|e| e.to_string())?;
    let path = DerivationPath::from_str("m/84'/0'/0'").map_err(|e| e.to_string())?;
    let account = master.derive_priv(secp, &path).map_err(|e| e.to_string())?;
    let wallet = SingleKeyWallet::new(master.fingerprint(secp), 0, Xpub::from_priv(secp, &account))
        .map_err(|e| e.to_string())?;
    Ok((wallet, account))
}

/// The float's account key, private, from its words.
fn float_account(secp: &Secp256k1<bitcoin::secp256k1::All>) -> Result<Xpriv, String> {
    let seed = bip39::Mnemonic::parse(WORDS)
        .map_err(|e| e.to_string())?
        .to_seed("");
    let master = Xpriv::new_master(Network::Bitcoin, &seed).map_err(|e| e.to_string())?;
    let path = DerivationPath::from_str("m/84'/0'/0'").map_err(|e| e.to_string())?;
    master.derive_priv(secp, &path).map_err(|e| e.to_string())
}

fn cmd_keys(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let a = parse_args(args)?;
    let (wallet, mut private) = match a.one("who")? {
        "float" => {
            let words = FloatWords::new(WORDS).map_err(|e| e.to_string())?;
            let w = FloatWallet::from_words(&words).map_err(|e| e.to_string())?;
            let single = SingleKeyWallet::parse(w.description()).map_err(|e| e.to_string())?;
            (single, float_account(&secp)?)
        }
        "shop" => shop(&secp)?,
        other => return Err(format!("--who {other}")),
    };
    println!("descriptor {}", wallet.canonical_string());
    // The same account key, private, under the test network's prefix.
    private.network = NetworkKind::Test;
    let public = wallet.xpub().to_string();
    let [r, c] = wallet.single_path_strings();
    for (name, text) in [("private-receive", &r), ("private-change", &c)] {
        let body = text.split('#').next().unwrap_or_default();
        println!("{name} {}", body.replace(&public, &private.to_string()));
    }
    for index in 0..8u32 {
        let script = wallet
            .script(Keychain::Receive, index)
            .map_err(|e| e.to_string())?;
        println!(
            "receive {index} {}",
            Address::from_script(&script, NETWORK).map_err(|e| e.to_string())?
        );
    }
    for index in 0..4u32 {
        let script = wallet
            .script(Keychain::Change, index)
            .map_err(|e| e.to_string())?;
        println!(
            "change {index} {}",
            Address::from_script(&script, NETWORK).map_err(|e| e.to_string())?
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn report(paid: &FloatPaid, out: &str) -> Result<(), String> {
    fs::write(out, serialize_hex(&paid.tx)).map_err(|e| e.to_string())?;
    println!("txid {}", paid.txid);
    println!("fee {}", paid.fee);
    println!("vsize {}", paid.vsize);
    println!("paid {}", paid.paid);
    match paid.change {
        Some((index, sats)) => println!("change {index} {sats}"),
        None => println!("change none"),
    }
    println!("inputs {}", paid.spent.len());
    Ok(())
}

fn cmd_pay(args: &[String]) -> Result<ExitCode, String> {
    let a = parse_args(args)?;
    let coins = read_coins(a.one("coins")?)?;
    let to = Address::from_str(a.one("addr")?)
        .map_err(|e| e.to_string())?
        .require_network(NETWORK)
        .map_err(|e| e.to_string())?;
    let words = FloatWords::new(WORDS).map_err(|e| e.to_string())?;
    match pay_refund(
        &words,
        &coins,
        &to.script_pubkey(),
        a.number("sats")?,
        a.number("rate")?,
        a.small("change-index")?,
        1,
        a.small("lock-height")?,
    ) {
        Ok(paid) => {
            report(&paid, a.one("out")?)?;
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => refused(&e),
    }
}

fn cmd_sweep(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let a = parse_args(args)?;
    let coins = read_coins(a.one("coins")?)?;
    // The shop's pinned description, read as the owner phone reads it.
    let (shop_wallet, _) = shop(&secp)?;
    let pinned =
        ImportedWallet::read(shop_wallet.canonical_string()).map_err(|e| format!("{e:?}"))?;
    let to = match shop_receive_script(&pinned, a.small("shop-index")?) {
        Ok(s) => s,
        Err(e) => return refused(&e),
    };
    println!(
        "to {}",
        Address::from_script(&to, NETWORK).map_err(|e| e.to_string())?
    );
    let words = FloatWords::new(WORDS).map_err(|e| e.to_string())?;
    match sweep(
        &words,
        &coins,
        &to,
        a.number("rate")?,
        1,
        a.small("lock-height")?,
    ) {
        Ok(paid) => {
            report(&paid, a.one("out")?)?;
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => refused(&e),
    }
}

fn cmd_refill(args: &[String]) -> Result<ExitCode, String> {
    let a = parse_args(args)?;
    match refill_amount(
        a.number("cap")?,
        a.number("confirmed")?,
        a.number("unconfirmed")?,
    ) {
        Ok(sats) => {
            println!("refill {sats}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => refused(&e),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        eprintln!("float keys|pay|sweep|refill ...");
        return ExitCode::from(1);
    };
    let result = match cmd.as_str() {
        "keys" => cmd_keys(rest),
        "pay" => cmd_pay(rest),
        "sweep" => cmd_sweep(rest),
        "refill" => cmd_refill(rest),
        other => Err(format!("unknown command {other}")),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
