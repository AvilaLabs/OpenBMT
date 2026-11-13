//! The starter wallet on regtest, driven by `scripts/starter-e2e-core.sh`
//! (`docs/START-SIMPLE-RULES.md`, SS-04, SS-05, SS-20, SS-30, SS-42, SS-43).
//!
//! TEST ONLY, NEVER USE: the kernel's bytes for the first read are given on
//! the command line, and the shop's key comes from fixed bytes, so anybody
//! can spend these wallets. Regtest only.
//!
//! The words are never printed. The script works them out on its own, with
//! its own code, from the same bytes; this program takes them only to type
//! the check-back on the keypad of the library, which passes only if they are
//! the library's words. Bitcoin Core plays the chain and the shop's new
//! wallet; the starter wallet's key is the same on both sides, under
//! different prefixes.
//!
//! ```text
//! starter wordlist
//! starter make --kernel HEX64 [--rolls DIGITS] --words "w1 .. w12"
//!     --sealed FILE --key FILE
//! starter shop-keys
//! starter pay --sealed FILE --key FILE --coins FILE --addr A --sats N
//!     --rate N --change-index N --lock-height N --out FILE
//! starter move --sealed FILE --key FILE --coins FILE --new-index N
//!     --rate N --lock-height N --min-conf N --out FILE
//! starter may-go --confirmations N
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
use openbmt_owner::starter::{
    move_all, pay_refund, seed_may_go, Coin, DataKey, KernelRandom, Key, Setup, StarterPaid,
    StarterRefusal, StarterWallet, KERNEL_BYTES,
};

const NETWORK: Network = Network::Regtest;

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

fn hex_to_bytes(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err("odd hex".to_string());
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The first two reads are the ones given (the second is the first with
/// every byte turned, so that the two differ); then the kernel's own.
struct GivenThenKernel {
    given: Vec<[u8; KERNEL_BYTES]>,
}

impl KernelRandom for GivenThenKernel {
    fn read32(&mut self) -> Option<[u8; KERNEL_BYTES]> {
        if !self.given.is_empty() {
            return Some(self.given.remove(0));
        }
        openbmt_kernel_random::read32().ok()
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

fn refused(e: &StarterRefusal) -> Result<ExitCode, String> {
    println!("refused {} {e}", e.rule());
    Ok(ExitCode::from(2))
}

/// The shop's new wallet, from fixed bytes, as the owner phone is given it
/// at the upgrade: the public description of one account, and the private
/// key for Core.
fn shop(secp: &Secp256k1<bitcoin::secp256k1::All>) -> Result<(SingleKeyWallet, Xpriv), String> {
    let master = Xpriv::new_master(Network::Bitcoin, &[0x77; 32]).map_err(|e| e.to_string())?;
    let path = DerivationPath::from_str("m/84'/0'/0'").map_err(|e| e.to_string())?;
    let account = master.derive_priv(secp, &path).map_err(|e| e.to_string())?;
    let wallet = SingleKeyWallet::new(master.fingerprint(secp), 0, Xpub::from_priv(secp, &account))
        .map_err(|e| e.to_string())?;
    Ok((wallet, account))
}

fn print_wallet(wallet: &SingleKeyWallet) -> Result<(), String> {
    println!("descriptor {}", wallet.canonical_string());
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
    Ok(())
}

fn cmd_wordlist() -> Result<ExitCode, String> {
    for w in bip39::Language::English.word_list() {
        println!("{w}");
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_shop_keys() -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let (wallet, mut private) = shop(&secp)?;
    print_wallet(&wallet)?;
    // The same account key, private, under the test network's prefix.
    private.network = NetworkKind::Test;
    let public = wallet.xpub().to_string();
    let [r, c] = wallet.single_path_strings();
    for (name, text) in [("private-receive", &r), ("private-change", &c)] {
        let body = text.split('#').next().unwrap_or_default();
        println!("{name} {}", body.replace(&public, &private.to_string()));
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_make(args: &[String]) -> Result<ExitCode, String> {
    let a = parse_args(args)?;
    let kernel: [u8; KERNEL_BYTES] = hex_to_bytes(a.one("kernel")?)?
        .try_into()
        .map_err(|_| "--kernel is 32 bytes of hex".to_string())?;
    let second = kernel.map(|b| b ^ 0x5a);
    let rolls: Vec<u8> =
        a.0.get("rolls")
            .map(|r| r.bytes().map(|b| b - b'0').collect())
            .unwrap_or_default();
    let mut rng = GivenThenKernel {
        given: vec![kernel, second],
    };
    let mut setup = match Setup::begin(&mut rng, &rolls) {
        Ok(s) => s,
        Err(e) => return refused(&e),
    };
    // The check-back, typed on the library's own keypad from the words the
    // script worked out: it passes only if they are the library's words.
    let words: Vec<&str> = a.one("words")?.split(' ').collect();
    let asked = match setup.start_check(&mut rng) {
        Ok(p) => p,
        Err(e) => return refused(&e),
    };
    for position in asked {
        let word = words
            .get(position - 1)
            .ok_or_else(|| "--words is 12 words".to_string())?;
        for b in word.bytes().take(4) {
            setup
                .key(Key::from_byte(b).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
        setup.key(Key::Enter).map_err(|e| e.to_string())?;
    }
    if !setup.passed() {
        println!("check-back failed: the words given are not the library's");
        return Ok(ExitCode::from(1));
    }
    println!("check-back passed on the keypad with the words the script worked out");
    let sealed = match setup.seal(&mut rng) {
        Ok(s) => s,
        Err(e) => return refused(&e),
    };
    fs::write(a.one("sealed")?, to_hex(&sealed.sealed)).map_err(|e| e.to_string())?;
    fs::write(a.one("key")?, to_hex(sealed.data_key.as_bytes())).map_err(|e| e.to_string())?;
    let wallet = StarterWallet::parse(setup.wallet().description()).map_err(|e| e.to_string())?;
    println!("fingerprint {}", wallet.fingerprint());
    print_wallet(&SingleKeyWallet::parse(wallet.description()).map_err(|e| e.to_string())?)?;
    Ok(ExitCode::SUCCESS)
}

fn read_sealed(a: &Args) -> Result<(Vec<u8>, DataKey), String> {
    let sealed = hex_to_bytes(
        fs::read_to_string(a.one("sealed")?)
            .map_err(|e| e.to_string())?
            .trim(),
    )?;
    let key = hex_to_bytes(
        fs::read_to_string(a.one("key")?)
            .map_err(|e| e.to_string())?
            .trim(),
    )?;
    Ok((
        sealed,
        DataKey::from_slice(&key).map_err(|e| e.to_string())?,
    ))
}

fn report(paid: &StarterPaid, out: &str) -> Result<(), String> {
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
    let (sealed, key) = read_sealed(&a)?;
    let coins = read_coins(a.one("coins")?)?;
    let to = Address::from_str(a.one("addr")?)
        .map_err(|e| e.to_string())?
        .require_network(NETWORK)
        .map_err(|e| e.to_string())?;
    match pay_refund(
        &sealed,
        &key,
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

fn cmd_move(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let a = parse_args(args)?;
    let (sealed, key) = read_sealed(&a)?;
    let coins = read_coins(a.one("coins")?)?;
    // The new wallet's pinned description, read as the owner phone reads it.
    let (shop_wallet, _) = shop(&secp)?;
    let new = ImportedWallet::read(shop_wallet.canonical_string()).map_err(|e| format!("{e:?}"))?;
    let index = a.small("new-index")?;
    let to = openbmt_owner::starter::receive_script_of(&new, index);
    if let Ok(script) = &to {
        println!(
            "to {}",
            Address::from_script(script, NETWORK).map_err(|e| e.to_string())?
        );
    }
    match move_all(
        &sealed,
        &key,
        &coins,
        &new,
        index,
        a.number("rate")?,
        a.small("min-conf")?,
        a.small("lock-height")?,
    ) {
        Ok(paid) => {
            report(&paid, a.one("out")?)?;
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => refused(&e),
    }
}

fn cmd_may_go(args: &[String]) -> Result<ExitCode, String> {
    let a = parse_args(args)?;
    println!(
        "may-go {}",
        if seed_may_go(a.small("confirmations")?) {
            "yes"
        } else {
            "no"
        }
    );
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        eprintln!("starter wordlist|make|shop-keys|pay|move|may-go ...");
        return ExitCode::from(1);
    };
    let result = match cmd.as_str() {
        "wordlist" => cmd_wordlist(),
        "make" => cmd_make(rest),
        "shop-keys" => cmd_shop_keys(),
        "pay" => cmd_pay(rest),
        "move" => cmd_move(rest),
        "may-go" => cmd_may_go(rest),
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
