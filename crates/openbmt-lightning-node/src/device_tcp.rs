//! The second device over TCP: a small framed protocol, so that a device on
//! another counter, the Box or the owner phone can keep the copy, and so that
//! a test can switch the device off and on.
//!
//! A frame is a 4-byte big-endian length and that many bytes. A request body
//! is [`MAGIC_SHOP_DEVICE`], one byte for the operation, a 2-byte length of
//! the name, the name, and for a write the blob. A reply body is one status
//! byte and the payload: the blob for a read, the names joined by a line
//! break for a list, the reason for a refusal. One request on a connection
//! gets one reply; the client opens a connection for each call, so a device
//! that is restarted is found again at once.
//!
//! The link is not encrypted: the blobs are sealed already (`seal.rs`), and
//! the device is on the shop's own network.

use crate::device::{name_is_valid, DeviceError, DeviceFuture, SecondDevice};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The four bytes every request of the device protocol starts with.
pub const MAGIC_SHOP_DEVICE: [u8; 4] = *b"OBM^";
/// The largest frame either side accepts, in bytes.
pub const MAX_FRAME: usize = 64 * 1024 * 1024;

const OP_WRITE: u8 = 1;
const OP_READ: u8 = 2;
const OP_LIST: u8 = 3;
const OP_REMOVE: u8 = 4;

const ST_OK: u8 = 0;
const ST_NONE: u8 = 1;
const ST_REFUSED: u8 = 2;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REPLY_TIMEOUT: Duration = Duration::from_secs(20);

async fn read_frame(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let len = stream.read_u32().await? as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame"));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    Ok(body)
}

async fn write_frame(stream: &mut TcpStream, body: &[u8]) -> std::io::Result<()> {
    stream.write_u32(body.len() as u32).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

fn request(op: u8, name: &str, blob: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(7 + name.len() + blob.len());
    body.extend_from_slice(&MAGIC_SHOP_DEVICE);
    body.push(op);
    body.extend_from_slice(&(name.len() as u16).to_be_bytes());
    body.extend_from_slice(name.as_bytes());
    body.extend_from_slice(blob);
    body
}

/// A second device reached over TCP.
#[derive(Debug, Clone)]
pub struct TcpDevice {
    addr: SocketAddr,
}

impl TcpDevice {
    /// A device listening at `addr`.
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }
}

async fn call(addr: SocketAddr, body: Vec<u8>) -> Result<(u8, Vec<u8>), DeviceError> {
    let away = |e: &dyn std::fmt::Display| DeviceError(format!("away: {e}"));
    let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| away(&"connect"))?
        .map_err(|e| away(&e))?;
    let exchange = async {
        write_frame(&mut stream, &body).await?;
        read_frame(&mut stream).await
    };
    let reply = tokio::time::timeout(REPLY_TIMEOUT, exchange)
        .await
        .map_err(|_| away(&"reply"))?
        .map_err(|e| away(&e))?;
    match reply.split_first() {
        Some((status, payload)) => Ok((*status, payload.to_vec())),
        None => Err(DeviceError("empty reply".into())),
    }
}

fn refusal(payload: Vec<u8>) -> DeviceError {
    DeviceError(String::from_utf8_lossy(&payload).into_owned())
}

impl SecondDevice for TcpDevice {
    fn write(&self, name: &str, blob: Vec<u8>) -> DeviceFuture<()> {
        let (addr, body) = (self.addr, request(OP_WRITE, name, &blob));
        Box::pin(async move {
            match call(addr, body).await? {
                (ST_OK, _) => Ok(()),
                (_, payload) => Err(refusal(payload)),
            }
        })
    }

    fn read(&self, name: &str) -> DeviceFuture<Option<Vec<u8>>> {
        let (addr, body) = (self.addr, request(OP_READ, name, &[]));
        Box::pin(async move {
            match call(addr, body).await? {
                (ST_OK, blob) => Ok(Some(blob)),
                (ST_NONE, _) => Ok(None),
                (_, payload) => Err(refusal(payload)),
            }
        })
    }

    fn list(&self) -> DeviceFuture<Vec<String>> {
        let (addr, body) = (self.addr, request(OP_LIST, "", &[]));
        Box::pin(async move {
            match call(addr, body).await? {
                (ST_OK, payload) => Ok(String::from_utf8_lossy(&payload)
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()),
                (_, payload) => Err(refusal(payload)),
            }
        })
    }

    fn remove(&self, name: &str) -> DeviceFuture<()> {
        let (addr, body) = (self.addr, request(OP_REMOVE, name, &[]));
        Box::pin(async move {
            match call(addr, body).await? {
                (ST_OK, _) => Ok(()),
                (_, payload) => Err(refusal(payload)),
            }
        })
    }
}

/// What the device answers to one request body.
async fn answer(device: &dyn SecondDevice, body: &[u8]) -> Vec<u8> {
    let refused = |why: &str| {
        let mut r = vec![ST_REFUSED];
        r.extend_from_slice(why.as_bytes());
        r
    };
    if body.len() < 7 || body[..4] != MAGIC_SHOP_DEVICE {
        return refused("magic");
    }
    let op = body[4];
    let name_len = u16::from_be_bytes([body[5], body[6]]) as usize;
    if body.len() < 7 + name_len {
        return refused("length");
    }
    let name = match std::str::from_utf8(&body[7..7 + name_len]) {
        Ok(n) => n,
        Err(_) => return refused("name"),
    };
    let blob = &body[7 + name_len..];
    if op != OP_LIST && !name_is_valid(name) {
        return refused("name");
    }
    match op {
        OP_WRITE => match device.write(name, blob.to_vec()).await {
            Ok(()) => vec![ST_OK],
            Err(e) => refused(&e.0),
        },
        OP_READ => match device.read(name).await {
            Ok(Some(b)) => [&[ST_OK][..], &b].concat(),
            Ok(None) => vec![ST_NONE],
            Err(e) => refused(&e.0),
        },
        OP_LIST => match device.list().await {
            Ok(names) => [&[ST_OK][..], names.join("\n").as_bytes()].concat(),
            Err(e) => refused(&e.0),
        },
        OP_REMOVE => match device.remove(name).await {
            Ok(()) => vec![ST_OK],
            Err(e) => refused(&e.0),
        },
        _ => refused("operation"),
    }
}

async fn serve_connection(mut stream: TcpStream, device: Arc<dyn SecondDevice>) {
    while let Ok(body) = read_frame(&mut stream).await {
        let reply = answer(device.as_ref(), &body).await;
        if write_frame(&mut stream, &reply).await.is_err() {
            return;
        }
    }
}

/// Serves `device` on `listener` until the task is dropped. Every write is on
/// disk (for a directory device) before its reply is sent.
pub async fn serve(listener: TcpListener, device: Arc<dyn SecondDevice>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(serve_connection(stream, Arc::clone(&device)));
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DirDevice;
    use crate::testutil::temp_dir;

    async fn start(dir: &str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let device: Arc<dyn SecondDevice> = Arc::new(DirDevice::new(temp_dir(dir)).unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        (addr, tokio::spawn(serve(listener, device)))
    }

    #[tokio::test]
    async fn sl_20_the_tcp_device_stores_reads_lists_and_removes() {
        let (addr, server) = start("tcp-basic").await;
        let dev = TcpDevice::new(addr);
        assert_eq!(dev.read("k~x").await.unwrap(), None);
        dev.write("k~x", vec![9; 100_000]).await.unwrap();
        dev.write("z", vec![]).await.unwrap();
        assert_eq!(dev.read("k~x").await.unwrap().unwrap().len(), 100_000);
        assert_eq!(dev.read("z").await.unwrap(), Some(vec![]));
        assert_eq!(dev.list().await.unwrap(), vec!["k~x".to_string(), "z".to_string()]);
        dev.remove("k~x").await.unwrap();
        assert_eq!(dev.list().await.unwrap(), vec!["z".to_string()]);
        assert!(dev.write("../x", vec![1]).await.is_err());
        server.abort();
    }

    #[tokio::test]
    async fn sl_23_a_tcp_device_that_is_not_there_is_an_error_not_a_hang() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let dev = TcpDevice::new(addr);
        let started = std::time::Instant::now();
        assert!(dev.write("k", vec![1]).await.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn sl_23_a_restarted_tcp_device_is_found_again_with_its_data() {
        let dir = temp_dir("tcp-restart");
        let device: Arc<dyn SecondDevice> = Arc::new(DirDevice::new(&dir).unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, Arc::clone(&device)));
        let dev = TcpDevice::new(addr);
        dev.write("k", vec![1, 2, 3]).await.unwrap();
        server.abort();
        let _ = server.await;
        assert!(dev.read("k").await.is_err());
        let listener = TcpListener::bind(addr).await.unwrap();
        let server = tokio::spawn(serve(listener, device));
        assert_eq!(dev.read("k").await.unwrap(), Some(vec![1, 2, 3]));
        server.abort();
    }

    #[tokio::test]
    async fn sl_20_a_request_without_the_magic_is_refused() {
        let (addr, server) = start("tcp-magic").await;
        let mut s = TcpStream::connect(addr).await.unwrap();
        write_frame(&mut s, b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
        let reply = read_frame(&mut s).await.unwrap();
        assert_eq!(reply[0], ST_REFUSED);
        server.abort();
    }
}
