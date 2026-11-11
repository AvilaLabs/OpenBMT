//! A hardware signer as key 2 on regtest, driven by
//! `scripts/hw-key2-e2e-core.sh`.
//!
//! TEST ONLY, NEVER USE: the three keys come from fixed test bytes (`[n; 32]`
//! for key n), as in the spend example, so anybody can spend this wallet.
//! Regtest only.
//!
//! ```text
//! hw_key2 keys
//! hw_key2 export --dir DIR --coins FILE --dest ADDRESS --sats N --rate N
//!     --lock-height N --change-index N --now SECONDS
//! hw_key2 take --dir DIR --kind file|b64|bbqr|ur|txn --in FILE --now SECONDS
//! ```
//!
//! - `keys` prints the wallet and, for key 2, the private descriptors a Core
//!   wallet imports to play the hardware signer.
//! - `export` is the owner phone: it queues a cash-out, builds the session
//!   for a hardware signer (the spends apart from the Key app's), signs with
//!   key 1, and writes `DIR/file.psbt`, `DIR/file.bbqr` (one part a line)
//!   and `DIR/file.ur` (one part a line), with the owner's state in `DIR`.
//! - `take` is what comes back: `file` is a binary PSBT, `b64` a base64
//!   one, `bbqr` and `ur` send the PSBT through the parts of that transport
//!   (in reverse order for BBQr) and read it back, `txn` is a finished
//!   transaction in hexadecimal. The finished transaction is written to
//!   `DIR/final.hex`.
//!
//! The output is one fact per line. Exit code 0 is success, 2 a refusal by
//! the library, 1 any other problem.

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::process::ExitCode;
use std::str::FromStr;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::consensus::encode::deserialize_hex;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::{All, Secp256k1};
use bitcoin::{Address, Network, OutPoint, Transaction, Txid};
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::Identity;
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_owner::{CashAmount, Delivery, Options, Owner, OwnerError};
use openbmt_signer::{Ceilings, FeeCeilings, SignerState, MAX_MONEY_SATS};
use openbmt_spend::Coin;

const NETWORK: Network = Network::Regtest;
const OWNER_SECRET: [u8; 32] = [0x11; 32];
const APPROVER_SECRET: [u8; 32] = [0x22; 32];

struct Keys {
    accounts: [Xpriv; 3],
    wallet: WalletDescription,
}

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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn cmd_keys() -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let k = keys(&secp)?;
    println!("descriptor {}", k.wallet.canonical_string());
    let [receive, change] = k.wallet.single_path_strings();
    println!("receive-descriptor {receive}");
    println!("change-descriptor {change}");
    for (name, text) in [("private-receive", &receive), ("private-change", &change)] {
        let body = text.split('#').next().unwrap_or_default();
        for n in 0..3 {
            let public = k.wallet.slots()[n].xpub.to_string();
            let private = k.accounts[n].to_string();
            println!("{name} {} {}", n + 1, body.replace(&public, &private));
        }
    }
    for index in 0..8u32 {
        let address = k
            .wallet
            .address(Keychain::Receive, index)
            .map_err(|e| e.to_string())?;
        println!("receive {index} {address}");
    }
    Ok(ExitCode::SUCCESS)
}

fn owner_of(k: &Keys, snapshot: Option<&[u8]>) -> Result<Owner, String> {
    let approver = Identity::from_secret_bytes(&APPROVER_SECRET)
        .map_err(|e| e.to_string())?
        .device_id();
    let policy = BatchPolicy { min_count: 3 };
    match snapshot {
        None => Owner::new(
            &OWNER_SECRET,
            k.wallet.clone(),
            approver,
            policy,
            Options::default(),
        )
        .map_err(|e| e.to_string()),
        Some(bytes) => Owner::restore(&OWNER_SECRET, k.wallet.clone(), approver, policy, bytes)
            .map_err(|e| e.to_string()),
    }
}

fn key1_state(k: &Keys, secp: &Secp256k1<All>, path: &str) -> Result<SignerState, String> {
    let public = Xpub::from_priv(secp, &k.accounts[0]);
    let key3 = Xpub::from_priv(secp, &k.accounts[2]);
    match fs::read(path) {
        Ok(bytes) => SignerState::from_bytes(&bytes, NETWORK, &public).map_err(|e| e.to_string()),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let mut s =
                SignerState::register(k.wallet.canonical_string(), NETWORK, &public, Some(&key3))
                    .map_err(|e| e.to_string())?;
            let all = Ceilings {
                per_tx: MAX_MONEY_SATS,
                per_day: MAX_MONEY_SATS,
                per_week: MAX_MONEY_SATS,
            };
            let store = |bytes: &[u8]| fs::write(path, bytes);
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
            Ok(s)
        }
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

fn random_for(now: u64) -> [u8; 96] {
    let mut out = [0u8; 96];
    for (i, chunk) in out.chunks_mut(32).enumerate() {
        let h = sha256::Hash::hash(format!("hw-key2/{now}/{i}").as_bytes()).to_byte_array();
        chunk.copy_from_slice(&h);
    }
    out
}

fn cmd_export(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let k = keys(&secp)?;
    let a = parse_args(args)?;
    let dir = a.one("dir")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let owner_path = format!("{dir}/owner.bin");
    let key1_path = format!("{dir}/key1.bin");
    let now = a.number("now")?;
    let coins = read_coins(a.one("coins")?)?;
    let dest = Address::from_str(a.one("dest")?)
        .map_err(|e| e.to_string())?
        .require_network(NETWORK)
        .map_err(|e| e.to_string())?;
    let mut owner = owner_of(&k, None)?;
    let mut key1 = key1_state(&k, &secp, &key1_path)?;
    let store = |b: &[u8]| fs::write(&owner_path, b);
    if let Err(e) = owner.queue_cash_out(
        dest.script_pubkey(),
        CashAmount::Sats(a.number("sats")?),
        "Cash-out",
        store,
    ) {
        return refused(e);
    }
    let lock = u32::try_from(a.number("lock-height")?).map_err(|_| "lock-height")?;
    let change = u32::try_from(a.number("change-index")?).map_err(|_| "change-index")?;
    let (built, signer) = match owner.build_request_for_signer(
        &coins,
        a.number("rate")?,
        change,
        lock,
        now,
        &random_for(now),
        store,
    ) {
        Ok(v) => v,
        Err(e) => return refused(e),
    };
    // The Key app's request holds no spend (AS-90).
    let key_app =
        openbmt_session::Request::from_bytes(&built.request).map_err(|e| e.to_string())?;
    let spends = key_app
        .items()
        .iter()
        .filter(|i| matches!(i.item, openbmt_session::Item::Spend { .. }))
        .count();
    println!("key-app-spends {spends}");
    for s in &built.skipped {
        println!("skipped {}", s.reason);
    }
    let session = signer.ok_or("no spend session was made")?;
    println!("session {}", hex(&session));
    fs::write(format!("{dir}/session.hex"), hex(&session)).map_err(|e| e.to_string())?;
    let account = k.accounts[0];
    let file = match owner.export_for_signer(&session, 0, &mut key1, &account, now, |o, key| {
        fs::write(&owner_path, o)?;
        fs::write(&key1_path, key)
    }) {
        Ok(f) => f,
        Err(e) => return refused(e),
    };
    println!("txid {}", file.txid);
    println!("fee {}", file.summary.fee);
    for p in &file.summary.payments {
        println!("pays {} {}", p.address, p.sats);
    }
    if let Some(c) = &file.summary.change {
        println!("change {} {}", c.address, c.sats);
    }
    println!("file-name {}", file.file_name);
    fs::write(format!("{dir}/file.psbt"), &file.psbt).map_err(|e| e.to_string())?;
    let parts = openbmt_bbqr::encode(openbmt_bbqr::Kind::Psbt, &file.psbt, 400)
        .map_err(|e| e.to_string())?;
    fs::write(format!("{dir}/file.bbqr"), parts.join("\n")).map_err(|e| e.to_string())?;
    println!("bbqr-parts {}", parts.len());
    let mut e = openbmt_ur::psbt_to_ur_parts(&file.psbt, 200).map_err(|e| e.to_string())?;
    let mut ur = Vec::new();
    for _ in 0..e.seq_len().max(1) {
        ur.push(e.next_part_upper());
    }
    fs::write(format!("{dir}/file.ur"), ur.join("\n")).map_err(|e| e.to_string())?;
    println!("ur-parts {}", ur.len());
    // The same file again, from the stored session, is the same bytes.
    let again = owner.signer_file(&session, 0).map_err(|e| e.to_string())?;
    println!("same-again {}", again.psbt == file.psbt);
    Ok(ExitCode::SUCCESS)
}

/// The bytes the transport carries back, read through that transport.
fn through(kind: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    match kind {
        "file" | "txn" | "b64" => Ok(bytes.to_vec()),
        "bbqr" => {
            let parts = openbmt_bbqr::encode(openbmt_bbqr::Kind::Psbt, bytes, 400)
                .map_err(|e| e.to_string())?;
            let mut j = openbmt_bbqr::Joiner::new(&[
                openbmt_bbqr::Kind::Psbt,
                openbmt_bbqr::Kind::Transaction,
            ]);
            for p in parts.iter().rev() {
                j.add(p).map_err(|e| e.to_string())?;
            }
            println!("bbqr-parts-back {}", parts.len());
            Ok(j.finish().map_err(|e| e.to_string())?.bytes)
        }
        "ur" => {
            let mut e = openbmt_ur::psbt_to_ur_parts(bytes, 200).map_err(|e| e.to_string())?;
            let mut d = openbmt_ur::Decoder::new();
            for _ in 0..(e.seq_len() * 4).max(2) {
                if d.receive(&e.next_part_upper())
                    .map_err(|e| e.to_string())?
                    .complete
                {
                    break;
                }
            }
            let (ty, cbor) = d.result().ok_or("the UR did not complete")?;
            println!("ur-type-back {ty}");
            openbmt_ur::ur_to_psbt(&ty, &cbor).map_err(|e| e.to_string())
        }
        other => Err(format!("kind {other}")),
    }
}

fn cmd_take(args: &[String]) -> Result<ExitCode, String> {
    let secp = Secp256k1::new();
    let k = keys(&secp)?;
    let a = parse_args(args)?;
    let dir = a.one("dir")?;
    let owner_path = format!("{dir}/owner.bin");
    let snapshot = fs::read(&owner_path).map_err(|e| e.to_string())?;
    let mut owner = owner_of(&k, Some(&snapshot))?;
    let session_hex =
        fs::read_to_string(format!("{dir}/session.hex")).map_err(|e| e.to_string())?;
    let session: [u8; 32] = (0..32)
        .map(|i| u8::from_str_radix(&session_hex[2 * i..2 * i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "session")?;
    let kind = a.one("kind")?;
    let input = fs::read(a.one("in")?).map_err(|e| e.to_string())?;
    let bytes = through(kind, &input)?;
    let taken = match owner.take_from_signer(&session, 0, &bytes, |b| fs::write(&owner_path, b)) {
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
    fs::write(format!("{dir}/final.hex"), hex(&tx)).map_err(|e| e.to_string())?;
    // The file the owner kept parses as a PSBT.
    let kept = owner.signer_file(&session, 0);
    println!("session-closed {}", kept.is_err());
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        eprintln!("usage: hw_key2 keys | export ... | take ...");
        return ExitCode::from(1);
    };
    let result = match command.as_str() {
        "keys" => cmd_keys(),
        "export" => cmd_export(rest),
        "take" => cmd_take(rest),
        other => Err(format!("unknown command {other}")),
    };
    result.unwrap_or_else(|e| {
        eprintln!("error: {e}");
        ExitCode::from(1)
    })
}
