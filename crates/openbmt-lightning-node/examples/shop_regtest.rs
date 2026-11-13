//! The shop's node on regtest, with a control port, for
//! `scripts/shop-lightning-e2e-core.sh` (SL-50).
//!
//! The node is made from the BIP-85 text's master key, the same words every
//! time, so the node id is the same every time. One command per connection on
//! 127.0.0.1; the reply ends when the connection closes.
//!
//! `shop_regtest ctl=<port> data=<dir> [rpc=<port> user=<u> pass=<p>]
//!  provider_id=<pubkey> provider_addr=<host:port> device=<port>
//!  [start_from=newer|local|device] [max_fee_msat=<n>] [index=<n>]
//!  [sweep_index=<n>] [wallet=<description>] [chain=filters p2p=<port>]
//!  [tor=<host:port>]`
//!
//! With `chain=filters` the node reads the chain from the peer at
//! `127.0.0.1:<p2p>` through compact filters, in `<data>/chain`, and uses no
//! RPC. With `tor=` the provider is reached through that SOCKS5 proxy only.
//!
//! The shop's pinned wallet is a single-key wallet made from the same master
//! key (account 0), unless `wallet=` gives a description. The node derives
//! every address it pays to from it (SL-33); the commands give indexes.
//!
//! Commands: `nodeid`, `status`, `quote <sats>`, `invoice <sats>`,
//! `invoice_capped <sats> <max_fee_msat>`, `paid`, `ack <hash>`,
//! `sweep <index> <sats>`, `sweep_index <index>`, `address <index>`,
//! `confirmations` (the funding transaction's, per channel),
//! `descriptor`, `close <index>`, `tracked`, `stop`.
//!
//! `invoice_capped` records a lower maximum for the provider's skim than the
//! provider will take, to see a payment refused for it (SL-29). `descriptor`
//! prints the wallet's receive descriptor with the test-network key prefix,
//! for a Core wallet that must agree on the addresses.
//!
//! `paid` lists the paid events waiting; each stays kept, and is given again
//! after a restart, until `ack` with its hash.

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::{PublicKey, Secp256k1};
use bitcoin::{Network, NetworkKind};
use openbmt_core::wallet::descriptor_checksum;
use openbmt_core::wallet_single::SingleKeyWallet;
use openbmt_lightning_node::bip85::node_seed;
use openbmt_lightning_node::chain::{ChainSource, CoreRpc};
use openbmt_lightning_node::device_tcp::TcpDevice;
use openbmt_lightning_node::filters::FilterConfig;
use openbmt_lightning_node::node::{Node, NodeConfig, Receive};
use openbmt_lightning_node::provider::ProviderConfig;
use openbmt_lightning_node::state::StartFrom;
use openbmt_net_io::DriverConfig;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

// The master key every vector of the BIP-85 text uses.
const XPRV: &str = "xprv9s21ZrQH143K2LBWUUQRFXhucrQqBpKdRRxNVq2zBqsx8HVqFk2uYo8kmbaLLHRdqtQpUm98uKfu3vca1LqdGhUtyoFnCNkfmXRyPXLjbKb";

#[tokio::main]
async fn main() {
    let args: HashMap<String, String> = std::env::args()
        .skip(1)
        .filter_map(|a| {
            a.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect();
    let get = |k: &str| args.get(k).unwrap_or_else(|| panic!("missing {k}")).clone();
    let master = Xpriv::from_str(XPRV).unwrap();
    let index: u32 = args.get("index").map(|v| v.parse().unwrap()).unwrap_or(0);
    let start_from = match args.get("start_from").map(String::as_str) {
        Some("local") => StartFrom::Local,
        Some("device") => StartFrom::Device,
        _ => StartFrom::Newer,
    };
    let config = NodeConfig {
        network: Network::Regtest,
        seed: node_seed(&master, index).unwrap(),
        data_dir: get("data").into(),
        chain: if args.get("chain").map(String::as_str) == Some("filters") {
            let mut driver = DriverConfig::new(Network::Regtest, 0);
            driver.allow_local = true;
            driver.fixed_peers = vec![format!("127.0.0.1:{}", get("p2p")).parse().unwrap()];
            ChainSource::Filters(FilterConfig {
                dir: std::path::PathBuf::from(get("data")).join("chain"),
                driver,
            })
        } else {
            ChainSource::Core(CoreRpc {
                host: "127.0.0.1".into(),
                port: get("rpc").parse().unwrap(),
                user: get("user"),
                password: get("pass"),
            })
        },
        tor: args.get("tor").map(|v| v.parse().unwrap()),
        device: Arc::new(TcpDevice::new(
            format!("127.0.0.1:{}", get("device")).parse().unwrap(),
        )),
        provider: ProviderConfig {
            node_id: PublicKey::from_str(&get("provider_id")).unwrap(),
            addr: get("provider_addr").parse().unwrap(),
            token: None,
            zero_conf: true,
        },
        max_opening_fee_msat: args
            .get("max_fee_msat")
            .map(|v| v.parse().unwrap())
            .unwrap_or(3_000_000),
        start_from,
        wallet: args
            .get("wallet")
            .cloned()
            .unwrap_or_else(|| wallet_text(&master)),
        sweep_index: args
            .get("sweep_index")
            .map(|v| v.parse().unwrap())
            .unwrap_or(0),
    };
    let node = match Node::start(config).await {
        Ok(n) => Arc::new(n),
        Err(e) => {
            println!("START_ERROR {e:?}");
            eprintln!("START_ERROR {e:?}");
            std::process::exit(3);
        }
    };
    eprintln!("started, node id {}", node.node_id());
    let listener = TcpListener::bind(format!("127.0.0.1:{}", get("ctl")))
        .await
        .unwrap();
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let node = Arc::clone(&node);
        tokio::spawn(async move {
            let (rd, mut wr) = stream.into_split();
            let mut line = String::new();
            if BufReader::new(rd).read_line(&mut line).await.is_err() {
                return;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            let stop = parts.first() == Some(&"stop");
            let out = if stop {
                "stopping".to_string()
            } else {
                handle(&node, &parts).await
            };
            let _ = wr.write_all(format!("{out}\n").as_bytes()).await;
            let _ = wr.flush().await;
            if stop {
                node.stop().await;
                std::process::exit(0);
            }
        });
    }
}

async fn handle(node: &Node, p: &[&str]) -> String {
    let sats = |i: usize| -> u64 { p.get(i).and_then(|v| v.parse().ok()).unwrap_or(0) };
    match p.first().copied().unwrap_or("") {
        "nodeid" => node.node_id().to_string(),
        "status" => {
            let s = node.status();
            format!(
                "device={:?} pending={} stored={} last_stored={} chain_ok={} height={} provider={} channels={} usable={} room_msat={}",
                s.device.state,
                s.device.pending_writes,
                s.device.stored_writes,
                s.device.last_stored_unix,
                s.chain_ok,
                s.chain_height,
                s.provider_connected,
                s.channels,
                s.usable_channels,
                s.room_msat
            )
        }
        "confirmations" => format!("{:?}", node.funding_confirmations()),
        "quote" => describe(&node.quote(sats(1) * 1000).await),
        "invoice" | "invoice_capped" => {
            let amount_msat = sats(1) * 1000;
            let mut quote = node.quote(amount_msat).await;
            if let (Some(cap), Receive::NewChannel { fee_msat, .. }) = (p.get(2), &mut quote) {
                *fee_msat = cap.parse().unwrap();
            }
            match node.invoice(amount_msat, &quote, "shop").await {
                Ok(made) => format!(
                    "invoice {} fee_msat={}",
                    made.invoice,
                    made.opening_fee_msat.unwrap_or(0)
                ),
                Err(e) => format!("onchain {e:?}"),
            }
        }
        "paid" => {
            let mut out = Vec::new();
            while let Some(paid) = node.try_paid() {
                let hash: String = paid
                    .payment_hash
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                out.push(format!(
                    "paid {hash} amount_msat={} at={}",
                    paid.amount_msat, paid.received_unix
                ));
            }
            out.join("\n")
        }
        "ack" => {
            let hash: Option<[u8; 32]> = p.get(1).and_then(|h| {
                (0..h.len() / 2)
                    .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok())
                    .collect::<Option<Vec<u8>>>()?
                    .try_into()
                    .ok()
            });
            match hash {
                Some(h) => format!("{:?}", node.ack_paid(h).await),
                None => "ERR ack <hash>".into(),
            }
        }
        "sweep_index" => format!("{:?}", node.set_sweep_index(sats(1) as u32).await),
        "address" => match node.wallet_address(sats(1) as u32) {
            Ok(a) => a.to_string(),
            Err(e) => format!("ERR {e}"),
        },
        "sweep" => format!("{:?}", node.sweep(sats(1) as u32, sats(2))),
        "tracked" => node.tracked_outputs().to_string(),
        "close" => format!("{:?}", node.close_all(sats(1) as u32)),
        "descriptor" => test_prefix_descriptor(&wallet_text(&Xpriv::from_str(XPRV).unwrap())),
        _ => "ERR unknown command".into(),
    }
}

/// The pinned wallet text of a single-key wallet at `84h/0h/0h` under `master`.
fn wallet_text(master: &Xpriv) -> String {
    let secp = Secp256k1::new();
    let path = DerivationPath::from_str("m/84h/0h/0h").unwrap();
    let account = master.derive_priv(&secp, &path).unwrap();
    SingleKeyWallet::new(
        master.fingerprint(&secp),
        0,
        Xpub::from_priv(&secp, &account),
    )
    .unwrap()
    .canonical_string()
    .to_string()
}

/// The receive descriptor of a wallet text with the test-network spelling of
/// the key, which is what Core on regtest reads, and the checksum.
fn test_prefix_descriptor(text: &str) -> String {
    let body = text.split('#').next().unwrap();
    let start = body.find(']').unwrap() + 1;
    let end = body[start..].find('/').unwrap() + start;
    let mut xpub = Xpub::from_str(&body[start..end]).unwrap();
    xpub.network = NetworkKind::Test;
    let body = format!("{}{}{}", &body[..start], xpub, &body[end..]).replace("/<0;1>/", "/0/");
    format!("{body}#{}", descriptor_checksum(&body).unwrap())
}

fn describe(r: &Receive) -> String {
    match r {
        Receive::Invoice => "invoice".into(),
        Receive::OnChainOnly(why) => format!("onchain {why:?}"),
        Receive::NewChannel { fee_msat, .. } => format!("newchannel fee_msat={fee_msat}"),
    }
}
