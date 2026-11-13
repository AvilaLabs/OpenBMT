//! An LSPS2 provider on ldk-node 0.7.1, with a line-based control port on
//! 127.0.0.1, for `scripts/shop-lightning-e2e-core.sh`.
//!
//! `shop-lightning-lsp dir=<datadir> ctl=<port> rpc=<port> user=<u> pass=<p>
//!  p2p=<port> [min_fee_msat=<n>]`
//!
//! The provider runs with `client_trusts_lsp` false (SL-28), takes 1% with a
//! minimum, funds the channel at six times the first payment, and allows a
//! client `to_self_delay` of up to 2016 blocks (SL-30). One command per
//! connection; the reply ends when the connection closes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use ldk_node::bitcoin::Network;
use ldk_node::lightning::ln::msgs::SocketAddress;
use ldk_node::liquidity::LSPS2ServiceConfig;
use ldk_node::logger::LogLevel;
use ldk_node::Builder;

fn main() {
    let args: HashMap<String, String> = std::env::args()
        .skip(1)
        .filter_map(|a| a.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect();
    let dir = PathBuf::from(&args["dir"]);
    std::fs::create_dir_all(&dir).unwrap();

    let mut b = Builder::new();
    b.set_network(Network::Regtest);
    b.set_storage_dir_path(dir.to_string_lossy().to_string());
    b.set_filesystem_logger(Some(dir.join("ldk.log").to_string_lossy().to_string()), Some(LogLevel::Debug));
    b.set_chain_source_bitcoind_rpc(
        "127.0.0.1".into(),
        args["rpc"].parse().unwrap(),
        args["user"].clone(),
        args["pass"].clone(),
    );
    b.set_listening_addresses(vec![SocketAddress::from_str(&format!("127.0.0.1:{}", args["p2p"])).unwrap()])
        .unwrap();
    b.set_liquidity_provider_lsps2(LSPS2ServiceConfig {
        require_token: None,
        advertise_service: false,
        channel_opening_fee_ppm: 10_000,
        // The channel is the forwarded amount times (1 + 5).
        channel_over_provisioning_ppm: 5_000_000,
        min_channel_opening_fee_msat: args.get("min_fee_msat").map(|v| v.parse().unwrap()).unwrap_or(1_000_000),
        min_channel_lifetime: 100,
        max_client_to_self_delay: 2016,
        min_payment_size_msat: 1_000_000,
        max_payment_size_msat: 1_000_000_000,
        client_trusts_lsp: false,
    });
    let node = Arc::new(b.build_with_fs_store().expect("build"));
    node.start().expect("start");
    eprintln!("[lsp] started, node id {}", node.node_id());

    let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    {
        let (n, ev) = (node.clone(), events.clone());
        std::thread::spawn(move || loop {
            let e = n.wait_next_event();
            let s = format!("{e:?}");
            eprintln!("[event] {s}");
            ev.lock().unwrap().push(s);
            let _ = n.event_handled();
        });
    }

    let l = TcpListener::bind(format!("127.0.0.1:{}", args["ctl"])).unwrap();
    for c in l.incoming().flatten() {
        let (node, events) = (node.clone(), events.clone());
        std::thread::spawn(move || {
            let mut line = String::new();
            let mut rd = BufReader::new(c.try_clone().unwrap());
            if rd.read_line(&mut line).is_err() {
                return;
            }
            let mut w = c;
            let parts: Vec<&str> = line.split_whitespace().collect();
            let out = handle(&node, &events, &parts);
            let _ = writeln!(w, "{out}");
            if parts.first() == Some(&"stop") {
                let _ = w.flush();
                std::process::exit(0);
            }
        });
    }
}

fn handle(node: &ldk_node::Node, events: &Mutex<Vec<String>>, p: &[&str]) -> String {
    match p.first().copied().unwrap_or("") {
        "nodeid" => node.node_id().to_string(),
        "addr" => node.onchain_payment().new_address().map(|a| a.to_string()).unwrap_or_else(|e| format!("ERR {e:?}")),
        "balances" => format!("{:?}", node.list_balances()),
        "channels" => node
            .list_channels()
            .iter()
            .map(|c| {
                format!(
                    "chan cp={} funding_txo={:?} value_sats={} outbound_msat={} inbound_msat={} ready={} usable={} announced={}",
                    c.counterparty_node_id,
                    c.funding_txo,
                    c.channel_value_sats,
                    c.outbound_capacity_msat,
                    c.inbound_capacity_msat,
                    c.is_channel_ready,
                    c.is_usable,
                    c.is_announced
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        // Force-closes every channel with the node whose id is given.
        "forceclose" => {
            let Some(id) = p.get(1).and_then(|v| ldk_node::bitcoin::secp256k1::PublicKey::from_str(v).ok()) else {
                return "ERR forceclose <node id>".into();
            };
            let results: Vec<String> = node
                .list_channels()
                .iter()
                .filter(|c| c.counterparty_node_id == id)
                .map(|c| format!("{:?}", node.force_close_channel(&c.user_channel_id, id, None)))
                .collect();
            results.join("\n")
        }
        "events" => std::mem::take(&mut *events.lock().unwrap()).join("\n"),
        "stop" => format!("{:?}", node.stop()),
        _ => "ERR unknown command".into(),
    }
}
