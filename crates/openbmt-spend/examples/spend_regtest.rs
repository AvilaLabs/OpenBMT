//! The spend path on regtest, driven by `scripts/spend-e2e-core.sh`.
//!
//! TEST ONLY, NEVER USE: the three keys of the wallet come from fixed test
//! bytes (`[n; 32]` for key n), so anybody can spend this wallet. Regtest
//! only. There is no real key here, and no way to give a real one.
//!
//! ```text
//! spend_regtest keys [index ...]
//! spend_regtest plan --coins FILE --rate N --change-index N --min-conf N
//!     --out FILE [--send-all] [--signed FILE ...] [--combined-out FILE]
//!     [--hex-out FILE] <address:sats> ...
//! spend_regtest sign --who 1|2|3 --state FILE --now SECONDS --in FILE --out FILE
//! spend_regtest forge --psbt FILE --hex-out FILE
//! ```
//!
//! - `keys` prints the wallet description, its two single-path forms, the
//!   same two with the private account key of key 1, 2 or 3 in place of its
//!   public key (for a Core wallet that holds that key), and the addresses
//!   at the indexes given.
//! - `plan` reads coins (one per line: `txid vout confirmations chain index
//!   previous-transaction-hex`, with chain 0 for receive and 1 for change),
//!   plans the payments, prints the plan and writes the PSBT. With `--signed`
//!   it joins those signed copies into the file, finishes it against the
//!   same plan and writes the transaction in hex.
//! - `sign` runs the real signer as key 1, 2 or 3: review, then sign. The
//!   state file is created on first use with every ceiling raised to the
//!   highest the signer takes, as the owner would do on the Key app's screen.
//! - `forge` is the negative path: it signs a file with keys 1 and 2 without
//!   the signer, puts a signature made for another input in place of one of
//!   the two on input 0, shows that `finalize` refuses it, and then builds
//!   the transaction by hand anyway, so that Core can be asked about it.
//!
//! The output is one fact per line. Exit code 0 is success, 2 a refusal by
//! the library or the signer, 1 any other problem.

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::process::ExitCode;
use std::str::FromStr;

use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv, Xpub};
use bitcoin::consensus::encode::{deserialize_hex, serialize_hex};
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{All, Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{ecdsa, Address, Amount, Network, OutPoint, Transaction, Txid, Witness};
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_signer::{Ceilings, FeeCeilings, SignError, SignerState, MAX_MONEY_SATS};
use openbmt_spend::{combine, finalize, finalize_plan, plan, Coin, SpendRequest};

const NETWORK: Network = Network::Regtest;

struct Keys {
    accounts: [Xpriv; 3],
    wallet: WalletDescription,
}

/// Three keys from fixed bytes, and the wallet they make. Test only.
fn keys(secp: &Secp256k1<All>) -> Result<Keys, String> {
    let path = DerivationPath::from_str("m/48'/1'/0'/2'").map_err(|e| e.to_string())?;
    let mut accounts = Vec::new();
    let mut slots = Vec::new();
    for n in 1..=3u8 {
        let master = Xpriv::new_master(NETWORK, &[n; 32]).map_err(|e| e.to_string())?;
        let account = master.derive_priv(secp, &path).map_err(|e| e.to_string())?;
        slots.push(KeySlot {
            index: n,
            origin_fingerprint: master.fingerprint(secp),
            origin_path: path.clone(),
            xpub: Xpub::from_priv(secp, &account),
        });
        accounts.push(account);
    }
    let slots: [KeySlot; 3] = slots.try_into().map_err(|_| "slots")?;
    let wallet = WalletDescription::new(NETWORK, slots).map_err(|e| e.to_string())?;
    let accounts: [Xpriv; 3] = accounts.try_into().map_err(|_| "accounts")?;
    Ok(Keys { accounts, wallet })
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
    let k = keys(&secp)?;
    println!("descriptor {}", k.wallet.canonical_string());
    let [receive, change] = k.wallet.single_path_strings();
    println!("receive-descriptor {receive}");
    println!("change-descriptor {change}");
    // The same two with one account key private. Core adds its own
    // checksum, so none is printed.
    for (name, text) in [("private-receive", &receive), ("private-change", &change)] {
        let body = text.split('#').next().unwrap_or_default();
        for n in 0..3 {
            let public = k.wallet.slots()[n].xpub.to_string();
            let private = k.accounts[n].to_string();
            if !body.contains(&public) {
                return Err("the descriptor does not hold the account key".to_string());
            }
            println!("{name} {} {}", n + 1, body.replace(&public, &private));
        }
    }
    for arg in args {
        let index: u32 = arg.parse().map_err(|_| format!("bad index: {arg}"))?;
        for (name, chain) in [("receive", Keychain::Receive), ("change", Keychain::Change)] {
            let address = k.wallet.address(chain, index).map_err(|e| e.to_string())?;
            println!("{name} {index} {address}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_plan(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let k = keys(&secp)?;
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
    };
    let planned = match plan(&k.wallet, &coins, &request) {
        Ok(p) => p,
        Err(e) => {
            println!("refused {e}");
            return Ok(ExitCode::from(2));
        }
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
    for p in planned.payments() {
        let address = Address::from_script(&p.script, NETWORK).map_err(|e| e.to_string())?;
        println!("payment {address} {}", p.sats);
    }
    if let Some(c) = planned.change() {
        let address = Address::from_script(&c.script, NETWORK).map_err(|e| e.to_string())?;
        println!("change {} {address} {}", c.index, c.sats);
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

    let Some(signed) = a.values.get("signed") else {
        return Ok(ExitCode::SUCCESS);
    };
    let mut copies = Vec::new();
    for path in signed {
        let bytes = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        copies.push(Psbt::deserialize(&bytes).map_err(|e| format!("{path}: {e}"))?);
    }
    let joined = match combine(&base, &copies) {
        Ok(j) => j,
        Err(e) => {
            println!("refused {e}");
            return Ok(ExitCode::from(2));
        }
    };
    if let Ok(path) = a.one("combined-out") {
        fs::write(path, joined.serialize()).map_err(|e| format!("{path}: {e}"))?;
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
        Err(e) => {
            println!("refused {e}");
            return Ok(ExitCode::from(2));
        }
    };
    // `finalize` and `finalize_plan` give the same transaction.
    if finalize(&k.wallet, &joined).as_ref() != Ok(&tx) {
        return Err("finalize and finalize_plan disagree".to_string());
    }
    println!("final-txid {}", tx.compute_txid());
    println!("final-weight {}", tx.weight().to_wu());
    println!("final-vsize {}", tx.weight().to_wu().div_ceil(4));
    let hex_out = a.one("hex-out")?;
    fs::write(hex_out, serialize_hex(&tx)).map_err(|e| format!("{hex_out}: {e}"))?;
    println!("finalized {}", tx.compute_txid());
    Ok(ExitCode::SUCCESS)
}

fn cmd_sign(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let k = keys(&secp)?;
    let a = parse_args(args, &[])?;
    let who = usize::try_from(a.number("who")?).map_err(|_| "who")?;
    if !(1..=3).contains(&who) {
        return Err("--who is 1, 2 or 3".to_string());
    }
    let now = a.number("now")?;
    let account = k.accounts[who - 1];
    let public = Xpub::from_priv(&secp, &account);
    let key3 = (who != 3).then(|| Xpub::from_priv(&secp, &k.accounts[2]));
    let state_path = a.one("state")?;
    let description = k.wallet.canonical_string();
    let mut state = match fs::read(state_path) {
        Ok(bytes) => SignerState::from_bytes(&bytes, NETWORK, &public)
            .map_err(|e| format!("state file: {e}"))?,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let mut s = SignerState::register(description, NETWORK, &public, key3.as_ref())
                .map_err(|e| format!("register: {e}"))?;
            // The owner's own decision on the Key app's screen: every
            // ceiling to the highest the signer takes.
            let all = Ceilings {
                per_tx: MAX_MONEY_SATS,
                per_day: MAX_MONEY_SATS,
                per_week: MAX_MONEY_SATS,
            };
            let store = |bytes: &[u8]| fs::write(state_path, bytes);
            s.set_ceilings(all, all, store)
                .map_err(|e| format!("ceilings: {e:?}"))?;
            s.set_fee_ceilings(
                FeeCeilings {
                    sat_per_vb: 1000,
                    absolute_sats: 2_000_000,
                },
                store,
            )
            .map_err(|e| format!("fee ceilings: {e:?}"))?;
            s
        }
        Err(e) => return Err(format!("state file: {e}")),
    };
    let input = a.one("in")?;
    let file = fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let review = match state.review(&file, now) {
        Ok(r) => r,
        Err(r) => {
            println!("refused {r}");
            return Ok(ExitCode::from(2));
        }
    };
    println!("txid {}", review.txid());
    println!("fee {}", review.fee_sats());
    println!("vsize {}", review.vsize());
    for o in review.outputs() {
        println!("output {} {} {:?}", o.index, o.amount, o.kind);
    }
    let signed = match state.sign(review, &account, now, |bytes| fs::write(state_path, bytes)) {
        Ok(s) => s,
        Err(SignError::Refused(r)) => {
            println!("refused {r}");
            return Ok(ExitCode::from(2));
        }
        Err(SignError::Persist(e)) => return Err(format!("state: {e}")),
    };
    let out = a.one("out")?;
    fs::write(out, &signed.psbt).map_err(|e| format!("{out}: {e}"))?;
    println!("signed {} by key {who}", signed.txid);
    Ok(ExitCode::SUCCESS)
}

/// The BIP 143 signature by key `who` (0, 1, 2) for input `of`: made with
/// that input's child key over that input's hash. Put on another input it
/// is a signature made for another input.
fn signature_for(
    secp: &Secp256k1<All>,
    k: &Keys,
    psbt: &Psbt,
    of: usize,
    who: usize,
) -> Result<ecdsa::Signature, String> {
    let input = psbt.inputs.get(of).ok_or("input")?;
    let (_, path) = input.bip32_derivation.values().next().ok_or("no hints")?;
    let steps: &[ChildNumber] = path.as_ref();
    let tail = steps.get(steps.len().saturating_sub(2)..).ok_or("path")?;
    let child = k.accounts[who]
        .derive_priv(secp, &tail)
        .map_err(|e| e.to_string())?;
    let ws = input.witness_script.as_ref().ok_or("witness script")?;
    let amount = input
        .witness_utxo
        .as_ref()
        .ok_or("witness utxo")?
        .value
        .to_sat();
    let mut cache = SighashCache::new(&psbt.unsigned_tx);
    let hash = cache
        .p2wsh_signature_hash(of, ws, Amount::from_sat(amount), EcdsaSighashType::All)
        .map_err(|e| e.to_string())?;
    Ok(ecdsa::Signature {
        signature: secp.sign_ecdsa(
            &Message::from_digest(hash.to_byte_array()),
            &child.private_key,
        ),
        sighash_type: EcdsaSighashType::All,
    })
}

fn cmd_forge(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let k = keys(&secp)?;
    let a = parse_args(args, &[])?;
    let path = a.one("psbt")?;
    let mut psbt = Psbt::deserialize(&fs::read(path).map_err(|e| format!("{path}: {e}"))?)
        .map_err(|e| e.to_string())?;
    if psbt.inputs.len() < 2 {
        return Err("the file needs two inputs".to_string());
    }
    // Keys 1 and 2 sign every input. On input 0, key 2's signature is the
    // one made for input 1.
    let mut keyed: Vec<Vec<(bitcoin::PublicKey, ecdsa::Signature)>> = Vec::new();
    for i in 0..psbt.inputs.len() {
        let hints: Vec<_> = psbt.inputs[i].bip32_derivation.keys().copied().collect();
        let mut pairs = Vec::new();
        for who in [0usize, 1] {
            let of = if i == 0 && who == 1 { 1 } else { i };
            let sig = signature_for(&secp, &k, &psbt, of, who)?;
            // The key of slot `who` among the three hints of input i.
            let (chain, index) = {
                let (_, p) = psbt.inputs[i]
                    .bip32_derivation
                    .values()
                    .next()
                    .ok_or("no hints")?;
                let steps: &[ChildNumber] = p.as_ref();
                let [.., ChildNumber::Normal { index: c }, ChildNumber::Normal { index }] = steps
                else {
                    return Err("path".to_string());
                };
                let chain = if *c == 1 {
                    Keychain::Change
                } else {
                    Keychain::Receive
                };
                (chain, *index)
            };
            let public = k
                .wallet
                .derived_keys(chain, index)
                .map_err(|e| e.to_string())?[who];
            if !hints.contains(&public.inner) {
                return Err("a hint is missing".to_string());
            }
            pairs.push((public, sig));
        }
        keyed.push(pairs);
    }
    for (input, pairs) in psbt.inputs.iter_mut().zip(&keyed) {
        for (key, sig) in pairs {
            input.partial_sigs.insert(*key, *sig);
        }
    }
    // Our finalize must refuse it.
    match finalize(&k.wallet, &psbt) {
        Ok(_) => return Err("finalize accepted a signature made for another input".to_string()),
        Err(e) => println!("refused {e}"),
    }
    // Forced out by hand: the witness in key order, no check.
    let mut tx = psbt.unsigned_tx.clone();
    for (txin, (input, pairs)) in tx.input.iter_mut().zip(psbt.inputs.iter().zip(&keyed)) {
        let ws = input.witness_script.as_ref().ok_or("witness script")?;
        let script = ws.as_bytes();
        let mut ordered = pairs.clone();
        ordered.sort_by_key(|(key, _)| {
            script
                .windows(33)
                .position(|w| w == key.inner.serialize())
                .unwrap_or(usize::MAX)
        });
        let mut witness = Witness::new();
        witness.push([] as [u8; 0]);
        for (_, sig) in &ordered {
            witness.push(sig.to_vec());
        }
        witness.push(ws.as_bytes());
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
        eprintln!("usage: spend_regtest keys|plan|sign|forge ...");
        return ExitCode::from(1);
    };
    let result = match command.as_str() {
        "keys" => cmd_keys(rest),
        "plan" => cmd_plan(rest),
        "sign" => cmd_sign(rest),
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
