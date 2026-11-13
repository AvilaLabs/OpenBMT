//! A second device for Shop Lightning, for tests: keeps the sealed blobs of
//! the node's state in a directory and answers over TCP on 127.0.0.1.
//!
//! `shop-lightning-device <port> <directory>`

use openbmt_lightning_node::device::{DirDevice, SecondDevice};
use openbmt_lightning_node::device_tcp::serve;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let (port, dir) = match (args.next(), args.next()) {
        (Some(p), Some(d)) => (p, d),
        _ => {
            eprintln!("usage: shop-lightning-device <port> <directory>");
            std::process::exit(2);
        }
    };
    let device: Arc<dyn SecondDevice> = match DirDevice::new(&dir) {
        Ok(d) => Arc::new(d),
        Err(e) => {
            eprintln!("directory: {e}");
            std::process::exit(2);
        }
    };
    let listener = match tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("port: {e}");
            std::process::exit(2);
        }
    };
    eprintln!("device on 127.0.0.1:{port}");
    serve(listener, device).await;
}
