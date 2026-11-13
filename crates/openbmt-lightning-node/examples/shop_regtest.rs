//! The shop's node on regtest, with a control port, for
//! `scripts/shop-lightning-e2e-core.sh` (SL-50).
//!
//! The node is made from the BIP-85 text's master key, the same words every
//! time, so the node id is the same every time. One command per connection on
//! 127.0.0.1; the reply ends when the connection closes.
//!
//! `shop_regtest ctl=<port> data=<dir> rpc=<port> user=<u> pass=<p>
//!  provider_id=<pubkey> provider_addr=<host:port> device=<port>
//!  [start_from=newer|local|device] [max_fee_msat=<n>] [index=<n>]`
//!
//! Commands: `nodeid`, `status`, `quote <sats>`, `invoice <sats>`, `paid`,
//! `sweep <address> <sats>`, `close <address>`, `stop`.

use bitcoin::bip32::Xpriv;
use bitcoin::secp256k1::PublicKey;
use bitcoin::{Address, Network};
use openbmt_lightning_node::bip85::node_seed;
use openbmt_lightning_node::chain::CoreRpc;
use openbmt_lightning_node::device_tcp::TcpDevice;
use openbmt_lightning_node::node::{Node, NodeConfig, Receive};
use openbmt_lightning_node::provider::ProviderConfig;
use openbmt_lightning_node::state::StartFrom;
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
        core: CoreRpc {
            host: "127.0.0.1".into(),
            port: get("rpc").parse().unwrap(),
            user: get("user"),
            password: get("pass"),
        },
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
        "quote" => describe(&node.quote(sats(1) * 1000).await),
        "invoice" => {
            let amount_msat = sats(1) * 1000;
            let quote = node.quote(amount_msat).await;
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
                out.push(format!("paid {hash} amount_msat={}", paid.amount_msat));
            }
            out.join("\n")
        }
        "sweep" => {
            match Address::from_str(p[1]).and_then(|a| a.require_network(Network::Regtest)) {
                Ok(a) => format!("{:?}", node.sweep(&a, sats(2))),
                Err(e) => format!("ERR {e}"),
            }
        }
        "close" => {
            match Address::from_str(p[1]).and_then(|a| a.require_network(Network::Regtest)) {
                Ok(a) => format!("{:?}", node.close_all(&a)),
                Err(e) => format!("ERR {e}"),
            }
        }
        _ => "ERR unknown command".into(),
    }
}

fn describe(r: &Receive) -> String {
    match r {
        Receive::Invoice => "invoice".into(),
        Receive::OnChainOnly(why) => format!("onchain {why:?}"),
        Receive::NewChannel { fee_msat, .. } => format!("newchannel fee_msat={fee_msat}"),
    }
}
