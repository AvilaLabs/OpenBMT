//! The spends of a shop with its own wallet on regtest, driven by
//! `scripts/owner-own-spend-e2e-core.sh` (`docs/SESSION-RULES.md` section 6,
//! AS-100 to AS-107).
//!
//! TEST ONLY, NEVER USE: the keys come from fixed test bytes, so anybody can
//! spend these wallets. Regtest only.
//!
//! The wallet is one key (`--kind single`, the mainnet description of the
//! account key, as the owner phone holds it) or a 2-of-3 of which Core holds
//! two keys (`--kind multi`). Core plays the shop's wallet: it reads the
//! file, signs it, and may finish and send it.
//!
//! ```text
//! own_spend keys --kind K
//! own_spend queue --dir D --kind K --what cash|refund|merge --sats N [--addr A] [--sale N] [--change-index N]
//! own_spend build --dir D --kind K --coins FILE --rate N --lock-height N
//!     --change-index N --now N --picks 1,2
//! own_spend take --dir D --kind K --number N --in FILE --now N
//! own_spend forge --dir D --kind K --number N --out FILE
//! own_spend confirmed --dir D --kind K --txid HEX
//! ```
//!
//! The output is one fact per line. Exit code 0 is success, 2 a refusal by
//! the library, 1 any other problem. The owner's state is in `D/owner.bin`.

use std::collections::HashMap;
use std::fs;
use std::process::ExitCode;
use std::str::FromStr;

use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv, Xpub};
use bitcoin::consensus::encode::deserialize_hex;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{All, Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{ecdsa, Address, Network, NetworkKind, OutPoint, PublicKey, Transaction, Txid};
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::Identity;
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_owner::own_shop::own_approver;
use openbmt_owner::{CashAmount, Delivery, Options, Owner, OwnerError, SaleFacts, SaleKey};
use openbmt_spend::Coin;

const NETWORK: Network = Network::Regtest;
const OWNER_SECRET: [u8; 32] = [0x11; 32];

struct Setup {
    wallet: ImportedWallet,
    /// The keys that are private here: (account key, index in the wallet).
    accounts: Vec<Xpriv>,
}

fn setup(kind: &str, secp: &Secp256k1<All>) -> Result<Setup, String> {
    match kind {
        "single" => {
            let master =
                Xpriv::new_master(Network::Bitcoin, &[0x77; 32]).map_err(|e| e.to_string())?;
            let path = DerivationPath::from_str("m/84'/0'/0'").map_err(|e| e.to_string())?;
            let account = master.derive_priv(secp, &path).map_err(|e| e.to_string())?;
            let w =
                SingleKeyWallet::new(master.fingerprint(secp), 0, Xpub::from_priv(secp, &account))
                    .map_err(|e| e.to_string())?;
            Ok(Setup {
                wallet: ImportedWallet::Single(w),
                accounts: vec![account],
            })
        }
        "multi" => {
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
            let w = WalletDescription::new(NETWORK, slots).map_err(|e| e.to_string())?;
            Ok(Setup {
                wallet: ImportedWallet::Multisig(w),
                accounts,
            })
        }
        other => Err(format!("--kind {other}")),
    }
}

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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn policy() -> BatchPolicy {
    BatchPolicy { min_count: 3 }
}

fn owner_of(s: &Setup, snapshot: Option<&[u8]>) -> Result<Owner, String> {
    let approver = own_approver(&OWNER_SECRET).ok_or("no approver")?;
    match snapshot {
        None => Owner::new(
            &OWNER_SECRET,
            s.wallet.clone(),
            approver,
            policy(),
            Options::default(),
        )
        .map_err(|e| e.to_string()),
        Some(bytes) => Owner::restore(&OWNER_SECRET, s.wallet.clone(), approver, policy(), bytes)
            .map_err(|e| e.to_string()),
    }
}

/// The owner's state from `DIR/owner.bin`, or a new one.
fn load(s: &Setup, dir: &str) -> Result<Owner, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    match fs::read(format!("{dir}/owner.bin")) {
        Ok(b) => owner_of(s, Some(&b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => owner_of(s, None),
        Err(e) => Err(e.to_string()),
    }
}

fn refused<E: std::fmt::Display>(e: OwnerError<E>) -> Result<ExitCode, String> {
    match e {
        OwnerError::Refused(r) => {
            println!("refused {r}");
            Ok(ExitCode::from(2))
        }
        OwnerError::Store(s) => Err(format!("store: {s}")),
    }
}

fn counter() -> Result<SaleKey, String> {
    let id = Identity::from_secret_bytes(&[0x31; 32]).map_err(|e| e.to_string())?;
    Ok(SaleKey {
        counter: id.device_id(),
        index: 0,
    })
}

fn cmd_keys(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let a = parse_args(args)?;
    let kind = a.one("kind")?;
    let s = setup(kind, &secp)?;
    match &s.wallet {
        ImportedWallet::Single(w) => {
            println!("descriptor {}", w.canonical_string());
            // The same account key, private, under the test network's prefix.
            let mut private = s.accounts[0];
            private.network = NetworkKind::Test;
            let public = w.xpub().to_string();
            let [r, c] = w.single_path_strings();
            for (name, text) in [("private-receive", &r), ("private-change", &c)] {
                let body = text.split('#').next().unwrap_or_default();
                println!("{name} {}", body.replace(&public, &private.to_string()));
            }
            for index in 0..24u32 {
                let script = w
                    .script(Keychain::Receive, index)
                    .map_err(|e| e.to_string())?;
                let address = Address::from_script(&script, NETWORK).map_err(|e| e.to_string())?;
                println!("receive {index} {address}");
            }
            for index in 0..4u32 {
                let script = w
                    .script(Keychain::Change, index)
                    .map_err(|e| e.to_string())?;
                let address = Address::from_script(&script, NETWORK).map_err(|e| e.to_string())?;
                println!("change {index} {address}");
            }
        }
        ImportedWallet::Multisig(w) => {
            println!("descriptor {}", w.canonical_string());
            // Core holds keys 1 and 2; key 3 stays public.
            let [r, c] = w.single_path_strings();
            for (name, text) in [("private-receive", &r), ("private-change", &c)] {
                let mut body = text.split('#').next().unwrap_or_default().to_string();
                for (slot, account) in w.slots().iter().zip(&s.accounts).take(2) {
                    body = body.replace(&slot.xpub.to_string(), &account.to_string());
                }
                println!("{name} {body}");
            }
            for index in 0..24u32 {
                println!(
                    "receive {index} {}",
                    w.address(Keychain::Receive, index)
                        .map_err(|e| e.to_string())?
                );
            }
            for index in 0..4u32 {
                println!(
                    "change {index} {}",
                    w.address(Keychain::Change, index)
                        .map_err(|e| e.to_string())?
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_queue(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let a = parse_args(args)?;
    let dir = a.one("dir")?;
    let s = setup(a.one("kind")?, &secp)?;
    let mut owner = load(&s, dir)?;
    let path = format!("{dir}/owner.bin");
    let store = |b: &[u8]| fs::write(&path, b);
    let result = match a.one("what")? {
        "cash" => {
            let to = Address::from_str(a.one("addr")?)
                .map_err(|e| e.to_string())?
                .require_network(NETWORK)
                .map_err(|e| e.to_string())?;
            owner.queue_cash_out(
                to.script_pubkey(),
                CashAmount::Sats(a.number("sats")?),
                "Savings",
                store,
            )
        }
        "refund" => {
            let to = Address::from_str(a.one("addr")?)
                .map_err(|e| e.to_string())?
                .require_network(NETWORK)
                .map_err(|e| e.to_string())?;
            let mut sale = counter()?;
            sale.index = u32::try_from(a.number("sale")?).map_err(|_| "sale")?;
            let facts = SaleFacts {
                received_sats: 1_000_000,
                reached_refund_depth: true,
            };
            if let Err(e) = owner.set_sale_facts(sale, facts, |b| fs::write(&path, b)) {
                return refused(e);
            }
            owner.queue_refund(
                sale,
                "Mia",
                to.script_pubkey(),
                a.number("sats")?,
                None,
                |b| fs::write(&path, b),
            )
        }
        // The merge of AS-106: a cash-out of everything to the wallet's own
        // change address.
        "merge" => {
            let index = u32::try_from(a.number("change-index")?).map_err(|_| "change-index")?;
            let script = s.wallet.change_script(index).map_err(|e| e.to_string())?;
            owner.queue_cash_out(script, CashAmount::All, "Merge", store)
        }
        other => return Err(format!("--what {other}")),
    };
    match result {
        Ok(id) => {
            println!("queued {id}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => refused(e),
    }
}

fn random_for(now: u64, picks: &str) -> [u8; 32] {
    sha256::Hash::hash(format!("own-spend/{now}/{picks}").as_bytes()).to_byte_array()
}

fn session_of(dir: &str) -> Result<[u8; 32], String> {
    let text = fs::read_to_string(format!("{dir}/session.hex")).map_err(|e| e.to_string())?;
    (0..32)
        .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "session".to_string())
}

fn cmd_build(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let a = parse_args(args)?;
    let dir = a.one("dir")?;
    let s = setup(a.one("kind")?, &secp)?;
    let mut owner = load(&s, dir)?;
    let path = format!("{dir}/owner.bin");
    let coins = read_coins(a.one("coins")?)?;
    let now = a.number("now")?;
    let picks: Vec<u32> = a
        .one("picks")?
        .split(',')
        .map(|p| p.parse().map_err(|_| "picks".to_string()))
        .collect::<Result<_, _>>()?;
    let lock = u32::try_from(a.number("lock-height")?).map_err(|_| "lock-height")?;
    let change = u32::try_from(a.number("change-index")?).map_err(|_| "change-index")?;
    let built = match owner.build_own_spend(
        &coins,
        &picks,
        a.number("rate")?,
        change,
        lock,
        now,
        &random_for(now, a.one("picks")?),
        |b| fs::write(&path, b),
    ) {
        Ok(b) => b,
        Err(e) => return refused(e),
    };
    for sk in &built.skipped {
        println!("skipped {}", sk.reason);
    }
    let Some(session) = built.session else {
        println!("no-session");
        return Ok(ExitCode::from(2));
    };
    fs::write(format!("{dir}/session.hex"), hex(&session)).map_err(|e| e.to_string())?;
    println!("items {}", built.items.len());
    for item in &built.items {
        let file = match owner.own_file(&session, item.number, |b| fs::write(&path, b)) {
            Ok(f) => f,
            Err(e) => return refused(e),
        };
        fs::write(format!("{dir}/item-{}.psbt", item.number), &file.psbt)
            .map_err(|e| e.to_string())?;
        println!(
            "item {} txid {} fee {} refunds {:?} cash-out {:?}",
            item.number, file.txid, file.summary.fee, item.refunds, item.cash_out
        );
        for p in &file.summary.payments {
            println!("pays {} {}", p.address, p.sats);
        }
        if let Some(c) = &file.summary.change {
            println!("change {} {}", c.address, c.sats);
        }
        let again = owner
            .own_file(&session, item.number, |b| fs::write(&path, b))
            .map_err(|_| "own_file again")?;
        println!("same-again {}", again.psbt == file.psbt);
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_take(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let a = parse_args(args)?;
    let dir = a.one("dir")?;
    let s = setup(a.one("kind")?, &secp)?;
    let mut owner = load(&s, dir)?;
    let path = format!("{dir}/owner.bin");
    let session = session_of(dir)?;
    let number = u32::try_from(a.number("number")?).map_err(|_| "number")?;
    let input = fs::read(a.one("in")?).map_err(|e| e.to_string())?;
    let taken = match owner.take_own(&session, number, &input, |b| fs::write(&path, b)) {
        Ok(t) => t,
        Err(e) => return refused(e),
    };
    println!("taken {} fee {}", taken.txid, taken.fee);
    let Some(Delivery::Transaction { tx, txid, .. }) = owner
        .outbox()
        .iter()
        .find(|d| d.id == taken.id)
        .map(|d| d.what.clone())
    else {
        return Err("the outbox holds no transaction".to_string());
    };
    println!("txid {txid}");
    fs::write(format!("{dir}/final-{number}.hex"), hex(&tx)).map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}

/// A file signed by the wrong key: every signature is made with a key of
/// the test's own and put under the wallet's public key, as a file would
/// look that another key signed.
fn cmd_forge(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let a = parse_args(args)?;
    let dir = a.one("dir")?;
    let number = a.number("number")?;
    let mut psbt = Psbt::deserialize(
        &fs::read(format!("{dir}/item-{number}.psbt")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let wrong = Xpriv::new_master(Network::Bitcoin, &[0xee; 32]).map_err(|e| e.to_string())?;
    let sk = wrong
        .derive_priv(&secp, &[ChildNumber::Normal { index: 0 }])
        .map_err(|e| e.to_string())?
        .private_key;
    let tx = psbt.unsigned_tx.clone();
    let mut cache = SighashCache::new(&tx);
    for (i, input) in psbt.inputs.iter_mut().enumerate() {
        let spent = match (&input.witness_utxo, &input.non_witness_utxo) {
            (Some(o), _) => o.clone(),
            (None, Some(prev)) => prev.output[tx.input[i].previous_output.vout as usize].clone(),
            _ => return Err("no previous output".to_string()),
        };
        let digest = match &input.witness_script {
            Some(ws) => cache
                .p2wsh_signature_hash(i, ws, spent.value, EcdsaSighashType::All)
                .map_err(|e| e.to_string())?
                .to_byte_array(),
            None => cache
                .p2wpkh_signature_hash(i, &spent.script_pubkey, spent.value, EcdsaSighashType::All)
                .map_err(|e| e.to_string())?
                .to_byte_array(),
        };
        let sig = secp.sign_ecdsa(&Message::from_digest(digest), &sk);
        for (pk, _) in input.bip32_derivation.clone() {
            input.partial_sigs.insert(
                PublicKey::new(pk),
                ecdsa::Signature {
                    signature: sig,
                    sighash_type: EcdsaSighashType::All,
                },
            );
        }
    }
    fs::write(a.one("out")?, psbt.serialize()).map_err(|e| e.to_string())?;
    println!("forged");
    Ok(ExitCode::SUCCESS)
}

/// AS-105: the wallet sent the transaction itself and a block holds it.
fn cmd_confirmed(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let a = parse_args(args)?;
    let dir = a.one("dir")?;
    let s = setup(a.one("kind")?, &secp)?;
    let mut owner = load(&s, dir)?;
    let path = format!("{dir}/owner.bin");
    let txid = Txid::from_str(a.one("txid")?).map_err(|e| e.to_string())?;
    println!("watched {}", owner.own_exported_transactions().len());
    if let Err(e) = owner.mark_exported_confirmed(&txid, |b| fs::write(&path, b)) {
        return refused(e);
    }
    for c in owner.cash_outs() {
        println!("cash-out {} {:?}", c.id, c.stage);
    }
    for r in owner.refunds() {
        println!("refund {} {:?}", r.id, r.stage);
    }
    println!("sessions {}", owner.open_sessions().len());
    println!("watched-after {}", owner.own_exported_transactions().len());
    // The state written reads back and gives the same bytes.
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    let back = owner_of(&s, Some(&bytes))?;
    println!(
        "snapshot-same {}",
        back.snapshot().map_err(|e| e.to_string())? == bytes
    );
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        eprintln!("own_spend keys|queue|build|take|forge|confirmed ...");
        return ExitCode::from(1);
    };
    let result = match cmd.as_str() {
        "keys" => cmd_keys(rest),
        "queue" => cmd_queue(rest),
        "build" => cmd_build(rest),
        "take" => cmd_take(rest),
        "forge" => cmd_forge(rest),
        "confirmed" => cmd_confirmed(rest),
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
