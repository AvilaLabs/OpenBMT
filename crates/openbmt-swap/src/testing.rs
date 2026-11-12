//! A pretend swap service and a regtest Bitcoin Core, for the tests of this
//! crate and of the crates that drive it (`testing` feature; SW-90, SW-91).
//! Not for use outside tests.
//!
//! The service answers the pairs, create and status calls as HTTP bytes, in
//! the shapes the Boltz API v2 documents and a live service was read to
//! use. It holds the refund key and builds the swap tree with the code the
//! client checks. It has modes that go wrong in one way each. The regtest
//! harness starts a private node under a folder the caller names, on the
//! loopback address only, with no peer and no listening port.

#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

use std::path::{Path, PathBuf};
use std::process::Command;

use bitcoin::absolute::LockTime;
use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::hex::{DisplayHex, FromHex};
use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
use bitcoin::taproot::TaprootBuilder;
use bitcoin::transaction::Version;
use bitcoin::{
    Address, Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid,
    Witness,
};
use openbmt_json::{Limits, Value};
use openbmt_lightning::{bolt11, testing::Make};

use crate::keys::key_agg;
use crate::pairs::Terms;
use crate::reply::Quote;
use crate::tree::{claim_script, refund_script, KeyOrder, SwapTree};

/// What the pretend service gets wrong, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Nothing.
    Honest,
    /// The lockup address is the one of a tree with another refund key.
    WrongTree,
    /// The lockup address is the one made with the shop's key first.
    OtherKeyOrder,
    /// The lockup pays less than the reply promised.
    ShortLockup,
    /// The timeout is too near the chain tip for a claim.
    NearTimeout,
}

/// A swap the pretend service made.
#[derive(Debug, Clone)]
pub struct Offer {
    pub id: String,
    pub address: Address,
    pub promised_sats: u64,
    pub lockup_sats: u64,
    pub timeout: u32,
    pub invoice: String,
}

/// The pretend service.
pub struct Pretend {
    pub terms: Terms,
    pub mode: Mode,
    pub network: Network,
    /// The blocks between the chain tip and the timeout it promises.
    pub timeout_blocks: u32,
    /// What it believes the chain tip and the time are.
    pub tip: u32,
    pub now: u64,
    /// Answer with `Transfer-Encoding: chunked` instead of a length.
    pub chunked: bool,
    pub refund: SecretKey,
    pub offer: Option<Offer>,
    /// The status it reports, and the lockup it carries.
    pub status: String,
    pub lockup: Option<Transaction>,
    /// Every request it was sent, for a test to look at.
    pub seen: Vec<Vec<u8>>,
}

/// The live terms read on 2026-11-12, as the pretend service's defaults.
pub fn live_terms() -> Terms {
    Terms {
        min_sats: 2142,
        max_sats: 5_000_000,
        fee_ppm: 6000,
        claim_fee_sats: 234,
        lockup_fee_sats: 324,
    }
}

/// The percentage of a fee in parts per million, as a JSON decimal.
pub fn percent_text(ppm: u32) -> String {
    let (whole, frac) = (ppm / 10_000, ppm % 10_000);
    if frac == 0 {
        return format!("{whole}.0");
    }
    let frac = format!("{frac:04}");
    format!("{whole}.{}", frac.trim_end_matches('0'))
}

fn http(status: &str, body: &[u8], chunked: bool) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n").into_bytes();
    if chunked {
        out.extend_from_slice(b"Transfer-Encoding: chunked\r\n\r\n");
        for part in body.chunks(37) {
            out.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
            out.extend_from_slice(part);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"0\r\n\r\n");
    } else {
        out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        out.extend_from_slice(body);
    }
    out
}

fn error(code: &str, why: &str) -> Vec<u8> {
    http(code, format!("{{\"error\":\"{why}\"}}").as_bytes(), false)
}

impl Pretend {
    pub fn new(network: Network, tip: u32, now: u64) -> Self {
        Self {
            terms: live_terms(),
            mode: Mode::Honest,
            network,
            timeout_blocks: 144,
            tip,
            now,
            chunked: false,
            refund: SecretKey::from_slice(&[5; 32]).unwrap(),
            offer: None,
            status: "swap.created".to_string(),
            lockup: None,
            seen: Vec::new(),
        }
    }

    pub fn refund_public(&self) -> PublicKey {
        PublicKey::from_secret_key(&Secp256k1::new(), &self.refund)
    }

    /// The body of the reply to the pairs call.
    pub fn pairs_body(&self) -> String {
        let t = &self.terms;
        format!(
            "{{\"BTC\":{{\"BTC\":{{\"hash\":\"{}\",\"rate\":1,\"limits\":{{\"maximal\":{},\
             \"minimal\":{}}},\"fees\":{{\"percentage\":{},\"minerFees\":{{\"claim\":{},\
             \"lockup\":{}}}}}}},\"L-BTC\":{{\"hash\":\"{}\",\"rate\":1,\"limits\":\
             {{\"maximal\":5000000,\"minimal\":100}},\"fees\":{{\"percentage\":0.4,\
             \"minerFees\":{{\"claim\":20,\"lockup\":27}}}}}}}}}}",
            [1u8; 32].to_lower_hex_string(),
            t.max_sats,
            t.min_sats,
            percent_text(t.fee_ppm),
            t.claim_fee_sats,
            t.lockup_fee_sats,
            [2u8; 32].to_lower_hex_string(),
        )
    }

    fn address_for(&self, claim: &PublicKey, hash: &[u8; 32], timeout: u32) -> (Address, SwapTree) {
        let honest = SwapTree::build(
            claim,
            &self.refund_public(),
            hash,
            timeout,
            KeyOrder::RefundFirst,
        )
        .unwrap();
        let address = match self.mode {
            Mode::WrongTree => {
                let other = PublicKey::from_secret_key(
                    &Secp256k1::new(),
                    &SecretKey::from_slice(&[6; 32]).unwrap(),
                );
                SwapTree::build(claim, &other, hash, timeout, KeyOrder::RefundFirst)
                    .unwrap()
                    .address(self.network)
            }
            Mode::OtherKeyOrder => {
                let internal = key_agg(&[*claim, self.refund_public()]).unwrap();
                let info = TaprootBuilder::new()
                    .add_leaf(1, claim_script(claim, hash))
                    .unwrap()
                    .add_leaf(1, refund_script(&self.refund_public(), timeout))
                    .unwrap()
                    .finalize(&Secp256k1::new(), internal)
                    .unwrap();
                Address::p2tr_tweaked(info.output_key(), self.network)
            }
            _ => honest.address(self.network),
        };
        (address, honest)
    }

    /// The body of the reply to a create call, or the HTTP error.
    fn create(&mut self, body: &[u8]) -> Result<Vec<u8>, Vec<u8>> {
        let doc = openbmt_json::parse(body, &Limits::DEFAULT)
            .map_err(|_| error("400 Bad Request", "not json"))?;
        let names: Vec<&str> = doc
            .as_object()
            .unwrap_or(&[])
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        if names
            != [
                "from",
                "to",
                "invoiceAmount",
                "preimageHash",
                "claimPublicKey",
                "description",
            ]
        {
            return Err(error("400 Bad Request", "members"));
        }
        let amount = doc
            .get("invoiceAmount")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if amount < self.terms.min_sats || amount > self.terms.max_sats {
            return Err(error("400 Bad Request", "amount is out of limits"));
        }
        let hash: [u8; 32] = doc
            .get("preimageHash")
            .and_then(Value::as_str)
            .and_then(|s| Vec::<u8>::from_hex(s).ok())
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| error("400 Bad Request", "hash"))?;
        let claim = doc
            .get("claimPublicKey")
            .and_then(Value::as_str)
            .and_then(|s| Vec::<u8>::from_hex(s).ok())
            .and_then(|b| PublicKey::from_slice(&b).ok())
            .ok_or_else(|| error("400 Bad Request", "key"))?;
        let timeout = match self.mode {
            Mode::NearTimeout => self.tip + 10,
            _ => self.tip + self.timeout_blocks,
        };
        let (address, tree) = self.address_for(&claim, &hash, timeout);
        let quote = Quote {
            fee_ppm: self.terms.fee_ppm,
            lockup_fee_sats: self.terms.lockup_fee_sats,
        };
        let promised = quote.least_onchain(amount);
        let lockup = match self.mode {
            Mode::ShortLockup => promised - 1,
            _ => promised,
        };
        let mut make = Make::good(hash);
        make.network = bolt11::network_prefix(self.network).unwrap();
        make.msat = Some(amount * 1000);
        make.timestamp = self.now;
        make.expiry = Some(3600);
        make.description = Some("swap".to_string());
        let invoice = make.write();
        let offer = Offer {
            id: "pretendSwap01".to_string(),
            address: address.clone(),
            promised_sats: promised,
            lockup_sats: lockup,
            timeout,
            invoice: invoice.clone(),
        };
        let reply = format!(
            "{{\"id\":\"{}\",\"invoice\":\"{}\",\"swapTree\":{{\"claimLeaf\":{{\"version\":192,\
             \"output\":\"{}\"}},\"refundLeaf\":{{\"version\":192,\"output\":\"{}\"}}}},\
             \"lockupAddress\":\"{}\",\"refundPublicKey\":\"{}\",\"timeoutBlockHeight\":{},\
             \"onchainAmount\":{}}}",
            offer.id,
            invoice,
            tree.claim_leaf().as_bytes().to_lower_hex_string(),
            tree.refund_leaf().as_bytes().to_lower_hex_string(),
            address,
            self.refund_public().serialize().to_lower_hex_string(),
            timeout,
            promised,
        );
        self.offer = Some(offer);
        self.status = "swap.created".to_string();
        self.lockup = None;
        Ok(reply.into_bytes())
    }

    /// The body of the reply to a status call.
    pub fn status_body(&self) -> String {
        match &self.lockup {
            Some(tx) => format!(
                "{{\"status\":\"{}\",\"transaction\":{{\"id\":\"{}\",\"hex\":\"{}\"}}}}",
                self.status,
                tx.compute_txid(),
                serialize(tx).to_lower_hex_string()
            ),
            None => format!("{{\"status\":\"{}\"}}", self.status),
        }
    }

    /// Play the service: take the bytes of a request, give the bytes of
    /// the reply. A request that has any header other than the four of
    /// SW-80, or that is not for a call it knows, is answered with an error.
    pub fn serve(&mut self, request: &[u8], prefix: &str, host: &str) -> Vec<u8> {
        self.seen.push(request.to_vec());
        let Some(split) = request.windows(4).position(|w| w == b"\r\n\r\n") else {
            return error("400 Bad Request", "no end of headers");
        };
        let head = std::str::from_utf8(&request[..split]).unwrap();
        let body = &request[split + 4..];
        let mut lines = head.split("\r\n");
        let line = lines.next().unwrap();
        let mut names: Vec<String> = lines
            .map(|l| l.split(':').next().unwrap().to_ascii_lowercase())
            .collect();
        names.sort();
        let want: &[&str] = if line.starts_with("POST ") {
            &["connection", "content-length", "content-type", "host"]
        } else {
            &["connection", "host"]
        };
        if names != want || !head.contains(&format!("Host: {host}\r\n")) {
            return error("400 Bad Request", "headers");
        }
        let mut parts = line.split(' ');
        let (verb, target, version) = (
            parts.next().unwrap(),
            parts.next().unwrap_or(""),
            parts.next().unwrap_or(""),
        );
        let Some(path) = target.strip_prefix(prefix) else {
            return error("404 Not Found", "no such path");
        };
        if version != "HTTP/1.1" {
            return error("400 Bad Request", "version");
        }
        match (verb, path) {
            ("GET", "/v2/swap/reverse") => {
                http("200 OK", self.pairs_body().as_bytes(), self.chunked)
            }
            ("POST", "/v2/swap/reverse") => match self.create(body) {
                Ok(reply) => http("201 Created", &reply, self.chunked),
                Err(e) => e,
            },
            ("GET", p)
                if self
                    .offer
                    .as_ref()
                    .is_some_and(|o| p == format!("/v2/swap/{}", o.id)) =>
            {
                http("200 OK", self.status_body().as_bytes(), self.chunked)
            }
            _ => error("404 Not Found", "no such swap"),
        }
    }

    /// A lockup transaction that pays the offer's address, with an input
    /// that spends nothing real: for a test with no node.
    pub fn pretend_lockup(&self) -> Transaction {
        let offer = self.offer.as_ref().unwrap();
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::new(Txid::from_byte_array([0xab; 32]), 0),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: vec![
                TxOut {
                    value: Amount::from_sat(offer.lockup_sats),
                    script_pubkey: offer.address.script_pubkey(),
                },
                TxOut {
                    value: Amount::from_sat(123_456),
                    script_pubkey: ScriptBuf::new_op_return([1u8; 4]),
                },
            ],
        }
    }

    /// The service reports a status, with a lockup if there is one.
    pub fn report(&mut self, status: &str, lockup: Option<Transaction>) {
        self.status = status.to_string();
        if lockup.is_some() {
            self.lockup = lockup;
        }
    }
}

/// A private regtest node, under a folder the caller names.
pub struct Regtest {
    bin: PathBuf,
    dir: PathBuf,
    port: u16,
}

impl Regtest {
    /// Start a node in `dir` (which must not hold another node) with RPC on
    /// `port` of the loopback address. Mines 101 blocks to a new wallet
    /// `pretend`. The binaries are the ones in `~/.local/opt/bitcoin-core-32.1/bin`
    /// unless `CORE_BIN` names another folder.
    pub fn start(dir: &Path, port: u16) -> Result<Self, String> {
        let bin = match std::env::var_os("CORE_BIN") {
            Some(b) => PathBuf::from(b),
            None => PathBuf::from(std::env::var_os("HOME").ok_or("no HOME")?)
                .join(".local/opt/bitcoin-core-32.1/bin"),
        };
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let node = Self {
            bin,
            dir: dir.to_path_buf(),
            port,
        };
        let out = Command::new(node.bin.join("bitcoind"))
            .args([
                "-regtest",
                "-listen=0",
                "-connect=0",
                "-dnsseed=0",
                "-daemonwait",
            ])
            .args(["-fallbackfee=0.0001", "-txindex=1", "-dbcache=50"])
            .arg(format!("-datadir={}", dir.display()))
            .arg(format!("-rpcport={port}"))
            .output()
            .map_err(|e| format!("bitcoind did not start: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "bitcoind: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        node.cli(&["createwallet", "pretend"])?;
        let addr = node.new_address()?;
        node.cli(&["generatetoaddress", "101", &addr.to_string()])?;
        Ok(node)
    }

    pub fn cli(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new(self.bin.join("bitcoin-cli"))
            .arg("-regtest")
            .arg(format!("-datadir={}", self.dir.display()))
            .arg(format!("-rpcport={}", self.port))
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(format!(
                "{}: {}",
                args.first().unwrap_or(&""),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    pub fn new_address(&self) -> Result<Address, String> {
        let text = self.cli(&["getnewaddress"])?;
        text.parse::<Address<_>>()
            .map_err(|e| e.to_string())?
            .require_network(Network::Regtest)
            .map_err(|e| e.to_string())
    }

    pub fn height(&self) -> Result<u32, String> {
        self.cli(&["getblockcount"])?
            .parse()
            .map_err(|_| "height".to_string())
    }

    pub fn mine(&self, blocks: u32) -> Result<(), String> {
        let addr = self.new_address()?;
        self.cli(&["generatetoaddress", &blocks.to_string(), &addr.to_string()])?;
        Ok(())
    }

    /// Send `sats` to `address` from the node's wallet, and give the
    /// transaction (in the mempool, not in a block).
    pub fn fund(&self, address: &Address, sats: u64) -> Result<Transaction, String> {
        let amount = format!("{}.{:08}", sats / 100_000_000, sats % 100_000_000);
        let txid = self.cli(&["sendtoaddress", &address.to_string(), &amount])?;
        let hex = self.cli(&["getrawtransaction", &txid])?;
        let bytes = Vec::<u8>::from_hex(&hex).map_err(|e| e.to_string())?;
        deserialize(&bytes).map_err(|e| e.to_string())
    }

    /// Whether Core's `testmempoolaccept` allows the transaction, and why
    /// not if it does not.
    pub fn test_accept(&self, tx: &Transaction) -> Result<(), String> {
        let hex = serialize(tx).to_lower_hex_string();
        let reply = self.cli(&["testmempoolaccept", &format!("[\"{hex}\"]")])?;
        let doc =
            openbmt_json::parse(reply.as_bytes(), &Limits::DEFAULT).map_err(|e| e.to_string())?;
        let first = doc
            .as_array()
            .and_then(<[Value]>::first)
            .ok_or("no verdict")?;
        match first.get("allowed").and_then(Value::as_bool) {
            Some(true) => Ok(()),
            _ => Err(reply),
        }
    }

    pub fn broadcast(&self, tx: &Transaction) -> Result<Txid, String> {
        let hex = serialize(tx).to_lower_hex_string();
        let txid = self.cli(&["sendrawtransaction", &hex])?;
        txid.parse().map_err(|_| "txid".to_string())
    }

    /// The height of the block that holds `txid`, if it is in one.
    pub fn confirmed_height(&self, txid: &Txid) -> Result<Option<u32>, String> {
        let info = self.cli(&["getrawtransaction", &txid.to_string(), "true"])?;
        let doc =
            openbmt_json::parse(info.as_bytes(), &Limits::DEFAULT).map_err(|e| e.to_string())?;
        let Some(hash) = doc.get("blockhash").and_then(Value::as_str) else {
            return Ok(None);
        };
        let header = self.cli(&["getblockheader", hash])?;
        let doc =
            openbmt_json::parse(header.as_bytes(), &Limits::DEFAULT).map_err(|e| e.to_string())?;
        Ok(doc
            .get("height")
            .and_then(Value::as_u64)
            .and_then(|h| u32::try_from(h).ok()))
    }
}

impl Drop for Regtest {
    fn drop(&mut self) {
        let pid_file = self.dir.join("regtest/bitcoind.pid");
        let pid = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        let _ = self.cli(&["stop"]);
        let Some(pid) = pid else { return };
        for _ in 0..60 {
            if !Path::new(&format!("/proc/{pid}")).exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        // Only a process that is still the node is stopped by force.
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if comm.trim() == "bitcoind" {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
        }
    }
}
