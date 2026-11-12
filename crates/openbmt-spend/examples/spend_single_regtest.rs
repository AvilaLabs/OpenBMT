//! The spend path of a wallet with one key on regtest, driven by
//! `scripts/spend-single-e2e-core.sh` (SP-87).
//!
//! TEST ONLY, NEVER USE: the key comes from fixed test bytes, so anybody can
//! spend this wallet. Regtest only. There is no real key here.
//!
//! The library's wallet is the mainnet description of the key
//! (`wpkh([fp/84h/0h/0h]xpub/<0;1>/*)`), as the owner phone holds it. Core's
//! regtest wallet gets the same key with its private part under the test
//! network's prefix (`tprv`); the scripts do not depend on the network. Core
//! addresses are the regtest addresses of the same scripts.
//!
//! ```text
//! spend_single_regtest keys [index ...]
//! spend_single_regtest plan --coins FILE --rate N --change-index N --min-conf N
//!     --out FILE [--lock-height N] [--change-seed HEX] [--send-all]
//!     [--signed FILE ...] [--final FILE] [--hex-out FILE]
//!     <address:sats> ...
//! spend_single_regtest forge --psbt FILE --hex-out FILE
//! ```
//!
//! - `keys` prints the mainnet description, the two private single-path
//!   descriptions for Core, and the regtest addresses at the indexes given.
//! - `plan` reads coins (one per line: `txid vout confirmations chain index
//!   previous-transaction-hex`, chain 0 for receive and 1 for change), plans
//!   the payments, prints the plan and writes the PSBT. With `--signed` it
//!   joins those returned files into the file with `take_wallet_signature`,
//!   finishes it against the plan and writes the transaction in hex. With
//!   `--final` it reads a returned file whose every input is final as the
//!   finished transaction it holds and checks it against the file sent.
//! - `forge` is the negative path: it puts on input 0 a signature made for
//!   input 1, shows that `finalize` refuses it, and builds the transaction by
//!   hand anyway so that Core can be asked about it.
//!
//! The output is one fact per line. Exit code 0 is success, 2 a refusal by
//! the library, 1 any other problem.

use std::collections::HashMap;
use std::fs;
use std::process::ExitCode;
use std::str::FromStr;

use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv, Xpub};
use bitcoin::consensus::encode::{deserialize_hex, serialize_hex};
use bitcoin::hashes::Hash;
use bitcoin::hex::FromHex;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{All, Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{
    ecdsa, Address, Network, NetworkKind, OutPoint, PublicKey, Transaction, Txid, Witness,
};
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_spend::{
    check_finished, finalize, finalize_plan, plan, take_wallet_signature,
    transaction_of_final_file, ChangePosition, Coin, PlanOutput, SpendRequest,
};

/// The Core side of the test is regtest.
const NETWORK: Network = Network::Regtest;

struct Key {
    account: Xpriv,
    wallet: ImportedWallet,
}

/// The one key from fixed bytes, and the wallet it makes. Test only.
fn key(secp: &Secp256k1<All>) -> Result<Key, String> {
    let master = Xpriv::new_master(Network::Bitcoin, &[0x77; 32]).map_err(|e| e.to_string())?;
    let path = DerivationPath::from_str("m/84'/0'/0'").map_err(|e| e.to_string())?;
    let account = master.derive_priv(secp, &path).map_err(|e| e.to_string())?;
    let single = SingleKeyWallet::new(master.fingerprint(secp), 0, Xpub::from_priv(secp, &account))
        .map_err(|e| e.to_string())?;
    Ok(Key {
        account,
        wallet: ImportedWallet::Single(single),
    })
}

fn single(k: &Key) -> Result<&SingleKeyWallet, String> {
    match &k.wallet {
        ImportedWallet::Single(w) => Ok(w),
        ImportedWallet::Multisig(_) => Err("not a wallet with one key".to_string()),
    }
}

/// `--name value` pairs (a name may repeat), flags, and what is left.
struct Args {
    values: HashMap<String, Vec<String>>,
    flags: Vec<String>,
    rest: Vec<String>,
}

fn parse_args(args: &[String], flags: &[&str]) -> Result<Args, String> {
    let mut out = Args {
        values: HashMap::new(),
        flags: Vec::new(),
        rest: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--") {
            if flags.contains(&name) {
                out.flags.push(name.to_string());
            } else {
                let v = it.next().ok_or(format!("--{name} needs a value"))?;
                out.values
                    .entry(name.to_string())
                    .or_default()
                    .push(v.clone());
            }
        } else {
            out.rest.push(a.clone());
        }
    }
    Ok(out)
}

impl Args {
    fn one(&self, name: &str) -> Result<&str, String> {
        match self.values.get(name).map(Vec::as_slice) {
            Some([v]) => Ok(v),
            _ => Err(format!("--{name} is needed once")),
        }
    }
    fn optional(&self, name: &str) -> Result<Option<&str>, String> {
        match self.values.get(name).map(Vec::as_slice) {
            None => Ok(None),
            Some([v]) => Ok(Some(v)),
            _ => Err(format!("--{name} is allowed once")),
        }
    }
    fn number(&self, name: &str) -> Result<u64, String> {
        self.one(name)?
            .parse()
            .map_err(|_| format!("--{name} is not a number"))
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
        let txid = Txid::from_str(txid).map_err(|e| e.to_string())?;
        let vout: u32 = vout.parse().map_err(|_| "vout")?;
        coins.push(Coin {
            outpoint: OutPoint { txid, vout },
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

fn cmd_keys(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let k = key(&secp)?;
    let w = single(&k)?;
    println!("descriptor {}", w.canonical_string());
    // Core's wallet: the same key, private, under the test network's
    // prefix. Core adds its own checksum, so none is printed.
    let mut private = k.account;
    private.network = NetworkKind::Test;
    let public = w.xpub().to_string();
    for (name, text) in [
        ("private-receive", &w.single_path_strings()[0]),
        ("private-change", &w.single_path_strings()[1]),
    ] {
        let body = text.split('#').next().unwrap_or_default();
        if !body.contains(&public) {
            return Err("the description does not hold the account key".to_string());
        }
        println!("{name} {}", body.replace(&public, &private.to_string()));
    }
    for arg in args {
        let index: u32 = arg.parse().map_err(|_| format!("bad index: {arg}"))?;
        for (name, chain) in [("receive", Keychain::Receive), ("change", Keychain::Change)] {
            let script = w.script(chain, index).map_err(|e| e.to_string())?;
            let address = Address::from_script(&script, NETWORK).map_err(|e| e.to_string())?;
            println!("{name} {index} {address}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn refuse(e: impl std::fmt::Display) -> Result<ExitCode, String> {
    println!("refused {e}");
    Ok(ExitCode::from(2))
}

fn cmd_plan(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let k = key(&secp)?;
    let a = parse_args(args, &["send-all"])?;
    let coins = read_coins(a.one("coins")?)?;
    let mut payments = Vec::new();
    for item in &a.rest {
        let (address, sats) = item
            .split_once(':')
            .ok_or("a payment is <address>:<sats>")?;
        let address = Address::from_str(address)
            .map_err(|e| e.to_string())?
            .require_network(NETWORK)
            .map_err(|e| e.to_string())?;
        let sats: u64 = sats.parse().map_err(|_| "sats")?;
        payments.push((address.script_pubkey(), sats));
    }
    let request = SpendRequest {
        payments,
        fee_rate: a.number("rate")?,
        change_index: u32::try_from(a.number("change-index")?).map_err(|_| "change index")?,
        min_confirmations: u32::try_from(a.number("min-conf")?).map_err(|_| "min-conf")?,
        send_all: a.flags.iter().any(|f| f == "send-all"),
        must_spend: Vec::new(),
        lock_height: match a.optional("lock-height")? {
            Some(h) => h.parse().map_err(|_| "--lock-height is not a number")?,
            None => 0,
        },
        change_position: match a.optional("change-seed")? {
            Some(hex) => {
                let bytes = <[u8; 32]>::from_hex(hex)
                    .map_err(|_| "--change-seed is 64 hex digits".to_string())?;
                ChangePosition::Seed(bytes)
            }
            None => ChangePosition::Last,
        },
    };
    let planned = match plan(&k.wallet, &coins, &request) {
        Ok(p) => p,
        Err(e) => return refuse(e),
    };
    println!("txid {}", planned.txid());
    for i in planned.inputs() {
        let c = &i.coin;
        let chain = match c.chain {
            Keychain::Receive => 0,
            Keychain::Change => 1,
        };
        println!(
            "input {} {} {} {chain} {}",
            c.outpoint, i.amount_sats, c.confirmations, c.index
        );
    }
    println!("lock-height {}", planned.lock_height());
    for o in planned.outputs() {
        match o {
            PlanOutput::Payment(p) => {
                let address =
                    Address::from_script(&p.script, NETWORK).map_err(|e| e.to_string())?;
                println!("payment {address} {}", p.sats);
            }
            PlanOutput::Change(c) => {
                let address =
                    Address::from_script(&c.script, NETWORK).map_err(|e| e.to_string())?;
                println!("change {} {address} {}", c.index, c.sats);
            }
        }
    }
    if let Some(at) = planned.change_position() {
        println!("change-position {at}");
    }
    if let Some(d) = planned.dropped_change_sats() {
        println!("dropped {d}");
    }
    println!("fee {}", planned.fee_sats());
    println!("vsize {}", planned.vsize());
    println!("weight {}", planned.weight());
    println!("rate-per-kvb {}", planned.fee_rate_per_kvb());
    println!("total-in {}", planned.total_in_sats());
    println!("total-out {}", planned.total_out_sats());
    let base = planned.to_psbt(&k.wallet).map_err(|e| e.to_string())?;
    let out = a.one("out")?;
    fs::write(out, base.serialize()).map_err(|e| format!("{out}: {e}"))?;

    let read = |path: &String| -> Result<Psbt, String> {
        let bytes = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        Psbt::deserialize(&bytes).map_err(|e| format!("{path}: {e}"))
    };
    let tx = if let Some(path) = a.optional("final")? {
        // SP-83, SP-86: a returned file whose every input is final.
        let returned = read(&path.to_string())?;
        let Some(tx) = transaction_of_final_file(&returned) else {
            return refuse("the returned file is not final on every input");
        };
        match check_finished(&k.wallet, &base, &tx) {
            Ok(fee) => println!("checked-fee {fee}"),
            Err(e) => return refuse(e),
        }
        tx
    } else if let Some(signed) = a.values.get("signed") {
        let mut joined = base.clone();
        for path in signed {
            let returned = read(path)?;
            joined = match take_wallet_signature(&k.wallet, &joined, &returned) {
                Ok(j) => j,
                Err(e) => return refuse(e),
            };
        }
        println!(
            "signatures {}",
            joined
                .inputs
                .iter()
                .map(|i| i.partial_sigs.len().to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let tx = match finalize_plan(&k.wallet, &joined, &planned) {
            Ok(tx) => tx,
            Err(e) => return refuse(e),
        };
        // `finalize` and `finalize_plan` give the same transaction.
        if finalize(&k.wallet, &joined).as_ref() != Ok(&tx) {
            return Err("finalize and finalize_plan disagree".to_string());
        }
        tx
    } else {
        return Ok(ExitCode::SUCCESS);
    };
    println!("final-txid {}", tx.compute_txid());
    println!("final-weight {}", tx.weight().to_wu());
    println!("final-vsize {}", tx.weight().to_wu().div_ceil(4));
    let hex_out = a.one("hex-out")?;
    fs::write(hex_out, serialize_hex(&tx)).map_err(|e| format!("{hex_out}: {e}"))?;
    println!("finalized {}", tx.compute_txid());
    Ok(ExitCode::SUCCESS)
}

/// The chain and index in the one hint of input `of`.
fn place(psbt: &Psbt, of: usize) -> Result<[ChildNumber; 2], String> {
    let input = psbt.inputs.get(of).ok_or("input")?;
    let (_, path) = input.bip32_derivation.values().next().ok_or("no hint")?;
    let steps: &[ChildNumber] = path.as_ref();
    match steps {
        [.., c @ ChildNumber::Normal { .. }, i @ ChildNumber::Normal { .. }] => Ok([*c, *i]),
        _ => Err("path".to_string()),
    }
}

/// The signature by the key of input `key_of`, over the hash of input
/// `hash_of`: for `key_of == hash_of`, the right signature; otherwise one
/// made for another input.
fn signature_for(
    secp: &Secp256k1<All>,
    k: &Key,
    psbt: &Psbt,
    key_of: usize,
    hash_of: usize,
) -> Result<(PublicKey, ecdsa::Signature), String> {
    let child = k
        .account
        .derive_priv(secp, &place(psbt, key_of)?)
        .map_err(|e| e.to_string())?;
    let public = PublicKey::new(child.private_key.public_key(secp));
    let spent = psbt
        .inputs
        .get(hash_of)
        .and_then(|i| i.witness_utxo.as_ref())
        .ok_or("witness utxo")?;
    let mut cache = SighashCache::new(&psbt.unsigned_tx);
    let hash = cache
        .p2wpkh_signature_hash(
            hash_of,
            &spent.script_pubkey,
            spent.value,
            EcdsaSighashType::All,
        )
        .map_err(|e| e.to_string())?;
    let signature = secp.sign_ecdsa(
        &Message::from_digest(hash.to_byte_array()),
        &child.private_key,
    );
    Ok((
        public,
        ecdsa::Signature {
            signature,
            sighash_type: EcdsaSighashType::All,
        },
    ))
}

fn cmd_forge(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let k = key(&secp)?;
    let a = parse_args(args, &[])?;
    let path = a.one("psbt")?;
    let mut psbt = Psbt::deserialize(&fs::read(path).map_err(|e| format!("{path}: {e}"))?)
        .map_err(|e| e.to_string())?;
    if psbt.inputs.len() < 2 {
        return Err("the file needs two inputs".to_string());
    }
    // The key signs every input; on input 0 the signature is the one made
    // for input 1.
    let mut pairs: Vec<(PublicKey, ecdsa::Signature)> = Vec::new();
    for i in 0..psbt.inputs.len() {
        let hash_of = if i == 0 { 1 } else { i };
        pairs.push(signature_for(&secp, &k, &psbt, i, hash_of)?);
    }
    for (input, (public, sig)) in psbt.inputs.iter_mut().zip(&pairs) {
        input.partial_sigs.insert(*public, *sig);
    }
    // The finisher must refuse it.
    match finalize(&k.wallet, &psbt) {
        Ok(_) => return Err("finalize accepted a signature made for another input".to_string()),
        Err(e) => println!("refused {e}"),
    }
    // Forced out by hand: the witness of a key-hash input, no check.
    let mut tx = psbt.unsigned_tx.clone();
    for (txin, (public, sig)) in tx.input.iter_mut().zip(&pairs) {
        let mut witness = Witness::new();
        witness.push(sig.to_vec());
        witness.push(public.inner.serialize());
        txin.witness = witness;
    }
    let hex_out = a.one("hex-out")?;
    fs::write(hex_out, serialize_hex(&tx)).map_err(|e| format!("{hex_out}: {e}"))?;
    println!("forced {}", tx.compute_txid());
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        eprintln!("usage: spend_single_regtest keys|plan|forge ...");
        return ExitCode::from(1);
    };
    let result = match command.as_str() {
        "keys" => cmd_keys(rest),
        "plan" => cmd_plan(rest),
        "forge" => cmd_forge(rest),
        other => Err(format!("unknown command: {other}")),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
