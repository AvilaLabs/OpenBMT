//! A blocking client for a relay (LK-30 to LK-32).
//!
//! A [`Client`] is one connection. It asks the relay to sign a fresh nonce
//! when it connects, and refuses a relay whose key is not the one it was
//! given (LK-32). Every call that reads or empties a mailbox first takes a
//! challenge and signs it (LK-23). The relay closes an idle connection after
//! 30 seconds, so a device that waits longer connects again.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_net_io::socks::{self, SocksError, Target};

use crate::proto::{
    parse_header, Direction, ProtoError, Refusal, Request, Response, FRAME_HEADER_LEN,
};
use crate::random::os_random;
use crate::relay::{hello_message, sign_challenge, Action, TAG_HELLO};

/// How to reach the relay (LK-31). There is no fourth way: no domain name
/// and no certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// An address and port on the local network.
    Direct(SocketAddr),
    /// An address and port, through a SOCKS5 proxy, for a device that sends
    /// nothing around its Tor proxy (CR-87). The address is given to the
    /// proxy as an address, never as a name.
    Proxied {
        /// The proxy.
        proxy: SocketAddr,
        /// The relay's address.
        addr: SocketAddr,
    },
    /// A Tor onion service, by name, through a SOCKS5 proxy. The name is
    /// never resolved here; the proxy does it.
    Onion {
        /// The proxy.
        proxy: SocketAddr,
        /// The `.onion` name.
        host: String,
        /// The port.
        port: u16,
    },
}

/// Timeouts of a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientConfig {
    /// For opening the TCP connection.
    pub connect: Duration,
    /// For each read and each write.
    pub io: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            io: Duration::from_secs(20),
        }
    }
}

/// Why a call to a relay failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// The name is not a `.onion` name (LK-31).
    NotOnion(String),
    /// The randomness failed.
    Randomness,
    /// The TCP connection could not be made.
    Connect(io::ErrorKind),
    /// The SOCKS5 handshake failed.
    Socks(SocksError),
    /// A read or write failed or timed out.
    Io(io::ErrorKind),
    /// The relay's frame was bad.
    Proto(ProtoError),
    /// The relay's key is not the one this device was given (LK-32).
    WrongRelay(DeviceId),
    /// The relay's signature on the hello is not valid (LK-32).
    HelloSignature,
    /// The relay refused.
    Refused(Refusal),
    /// The relay answered with the wrong kind of frame.
    Unexpected,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotOnion(n) => write!(f, "LK-31: {n} is not an onion name"),
            Self::Randomness => write!(f, "LK-03: the randomness failed"),
            Self::Connect(k) => write!(f, "LK-31: could not connect: {k}"),
            Self::Socks(e) => write!(f, "LK-31: {e}"),
            Self::Io(k) => write!(f, "LK-30: {k}"),
            Self::Proto(e) => write!(f, "{e}"),
            Self::WrongRelay(d) => write!(f, "LK-32: this is not the relay we were given: {d:?}"),
            Self::HelloSignature => write!(f, "LK-32: the relay's signature is not valid"),
            Self::Refused(r) => write!(f, "{r}"),
            Self::Unexpected => write!(f, "LK-30: the relay answered the wrong question"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<ProtoError> for ClientError {
    fn from(e: ProtoError) -> Self {
        Self::Proto(e)
    }
}

fn io_err(e: &io::Error) -> ClientError {
    ClientError::Io(e.kind())
}

/// Send one request and read the one answer.
fn exchange(stream: &mut TcpStream, request: &Request) -> Result<Response, ClientError> {
    let bytes = request.encode()?;
    stream.write_all(&bytes).map_err(|e| io_err(&e))?;
    let mut header = [0u8; FRAME_HEADER_LEN];
    stream.read_exact(&mut header).map_err(|e| io_err(&e))?;
    let (_, len) = parse_header(&header, Direction::ToDevice)?;
    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + len);
    frame.extend_from_slice(&header);
    frame.resize(FRAME_HEADER_LEN + len, 0);
    let body = frame
        .get_mut(FRAME_HEADER_LEN..)
        .ok_or(ClientError::Unexpected)?;
    stream.read_exact(body).map_err(|e| io_err(&e))?;
    Ok(Response::decode(&frame)?)
}

/// What a fetch brought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The relay's number and the sealed bytes of each message, oldest
    /// first.
    pub items: Vec<(u64, Vec<u8>)>,
    /// Whether more are waiting.
    pub more: bool,
}

/// One connection to a relay.
#[derive(Debug)]
pub struct Client {
    stream: TcpStream,
    relay: DeviceId,
}

impl Client {
    /// Connect and check the relay's hello. With `expected`, a relay with
    /// another key is refused (LK-32); with `None`, any relay that signs its
    /// own hello is taken, and [`Client::relay`] says which.
    pub fn connect(
        route: &Route,
        expected: Option<&DeviceId>,
        cfg: &ClientConfig,
    ) -> Result<Self, ClientError> {
        let stream = match route {
            Route::Direct(addr) => TcpStream::connect_timeout(addr, cfg.connect)
                .map_err(|e| ClientError::Connect(e.kind()))?,
            Route::Proxied { proxy, addr } => {
                let mut seed = [0u8; 32];
                os_random(&mut seed).map_err(|_| ClientError::Randomness)?;
                let mut token = [0u8; 8];
                token.copy_from_slice(seed.get(..8).ok_or(ClientError::Randomness)?);
                socks::connect_isolated(
                    *proxy,
                    &Target::Addr(*addr),
                    cfg.connect,
                    Duration::from_secs(socks::HANDSHAKE_SECS),
                    Some(u64::from_le_bytes(token)),
                )
                .map_err(ClientError::Socks)?
            }
            Route::Onion { proxy, host, port } => {
                if !host.ends_with(".onion") || host.len() <= ".onion".len() {
                    return Err(ClientError::NotOnion(host.clone()));
                }
                let mut seed = [0u8; 32];
                os_random(&mut seed).map_err(|_| ClientError::Randomness)?;
                let mut token = [0u8; 8];
                token.copy_from_slice(seed.get(..8).ok_or(ClientError::Randomness)?);
                // A new circuit for every connection (LK-31).
                socks::connect_isolated(
                    *proxy,
                    &Target::Name(host.clone(), *port),
                    cfg.connect,
                    Duration::from_secs(socks::HANDSHAKE_SECS),
                    Some(u64::from_le_bytes(token)),
                )
                .map_err(ClientError::Socks)?
            }
        };
        stream
            .set_read_timeout(Some(cfg.io))
            .map_err(|e| io_err(&e))?;
        stream
            .set_write_timeout(Some(cfg.io))
            .map_err(|e| io_err(&e))?;
        let mut stream = stream;
        let mut nonce = [0u8; 32];
        os_random(&mut nonce).map_err(|_| ClientError::Randomness)?;
        match exchange(&mut stream, &Request::Hello { nonce })? {
            Response::Hello { relay, sig } => {
                if let Some(want) = expected {
                    if *want != relay {
                        return Err(ClientError::WrongRelay(relay));
                    }
                }
                if !relay.verify_tagged(TAG_HELLO, &hello_message(&nonce, &relay), &sig) {
                    return Err(ClientError::HelloSignature);
                }
                Ok(Self { stream, relay })
            }
            Response::Refused(r) => Err(ClientError::Refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// The relay's key, as it proved it in the hello.
    pub fn relay(&self) -> &DeviceId {
        &self.relay
    }

    fn call(&mut self, request: &Request) -> Result<Response, ClientError> {
        exchange(&mut self.stream, request)
    }

    fn challenge(&mut self) -> Result<[u8; 32], ClientError> {
        match self.call(&Request::Challenge)? {
            Response::Challenge { challenge } => Ok(challenge),
            Response::Refused(r) => Err(ClientError::Refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Make the reader's mailbox if it has none (LK-21).
    pub fn make_mailbox(&mut self, reader: &Identity) -> Result<(), ClientError> {
        let challenge = self.challenge()?;
        let sig = sign_challenge(reader, &challenge, &self.relay, Action::Make, 0);
        match self.call(&Request::MakeMailbox {
            reader: reader.device_id(),
            challenge,
            sig,
        })? {
            Response::Made => Ok(()),
            Response::Refused(r) => Err(ClientError::Refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Hand the relay a sealed message; the relay's number for it (LK-20).
    pub fn put(&mut self, sealed: &[u8]) -> Result<u64, ClientError> {
        match self.call(&Request::Put {
            sealed: sealed.to_vec(),
        })? {
            Response::Stored { id } => Ok(id),
            Response::Refused(r) => Err(ClientError::Refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Fetch what is after `cursor` (LK-24). Nothing is deleted.
    pub fn fetch(&mut self, reader: &Identity, cursor: u64) -> Result<Fetched, ClientError> {
        let challenge = self.challenge()?;
        let sig = sign_challenge(reader, &challenge, &self.relay, Action::Fetch, cursor);
        match self.call(&Request::Fetch {
            reader: reader.device_id(),
            challenge,
            sig,
            cursor,
        })? {
            Response::Messages { items, more } => Ok(Fetched { items, more }),
            Response::Refused(r) => Err(ClientError::Refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Say that everything up to `up_to` is taken; how many the relay
    /// dropped (LK-24).
    pub fn ack(&mut self, reader: &Identity, up_to: u64) -> Result<u32, ClientError> {
        let challenge = self.challenge()?;
        let sig = sign_challenge(reader, &challenge, &self.relay, Action::Ack, up_to);
        match self.call(&Request::Ack {
            reader: reader.device_id(),
            challenge,
            sig,
            up_to,
        })? {
            Response::Acked { dropped } => Ok(dropped),
            Response::Refused(r) => Err(ClientError::Refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Ask a relay that is a Box for the claim of its open invitation
    /// (LK-27). A relay with none refuses.
    pub fn claim(&mut self) -> Result<Vec<u8>, ClientError> {
        match self.call(&Request::Claim)? {
            Response::Claim { bytes } => Ok(bytes),
            Response::Refused(r) => Err(ClientError::Refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Hand a Box a sealed Store message and take its sealed Stored
    /// answer (LK-82). A relay that is not a Box refuses.
    pub fn store(&mut self, sealed: &[u8]) -> Result<Vec<u8>, ClientError> {
        match self.call(&Request::Store {
            sealed: sealed.to_vec(),
        })? {
            Response::StoreAnswer { sealed } => Ok(sealed),
            Response::Refused(r) => Err(ClientError::Refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Hand a Box the welcome and the first settings (LK-27, BX-15).
    pub fn welcome(&mut self, welcome: &[u8], settings: &[u8]) -> Result<(), ClientError> {
        match self.call(&Request::Welcome {
            welcome: welcome.to_vec(),
            settings: settings.to_vec(),
        })? {
            Response::Welcomed => Ok(()),
            Response::Refused(r) => Err(ClientError::Refused(r)),
            _ => Err(ClientError::Unexpected),
        }
    }
}
