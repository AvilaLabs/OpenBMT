//! Opening a connection and looking up a seed name. Both can take long, so
//! the driver runs each in a thread of its own and never waits for them.

use std::fmt;
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use crate::socks::{self, SocksError, Target};

/// Seconds a TCP connection may take to open.
pub const CONNECT_SECS: u64 = 10;
/// NR-72: seconds a seed lookup may take. A later answer is ignored.
pub const RESOLVE_SECS: u64 = 10;

/// Why a dial failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialError {
    /// The TCP connection failed.
    Connect(ErrorKind),
    /// NR-26: the connection through the proxy failed.
    Proxy(SocksError),
    /// A host name was given and no proxy is set. A name is never looked up
    /// by a dial (NR-26).
    NameWithoutProxy,
    /// The thread for the dial could not be started.
    Thread,
}

impl fmt::Display for DialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(k) => write!(f, "could not connect: {k}"),
            Self::Proxy(e) => write!(f, "{e}"),
            Self::NameWithoutProxy => write!(f, "a host name needs a proxy"),
            Self::Thread => write!(f, "could not start a thread for the connection"),
        }
    }
}

impl std::error::Error for DialError {}

/// Open a connection to the target, through the proxy when there is one
/// (NR-26). `token` is a fresh random number for this connection; the proxy
/// is given it so that it keeps the connection apart from the others
/// (NR-78).
pub fn dial(
    target: &Target,
    proxy: Option<SocketAddr>,
    token: u64,
) -> Result<TcpStream, DialError> {
    let connect = Duration::from_secs(CONNECT_SECS);
    match (proxy, target) {
        (Some(proxy), target) => socks::connect_isolated(
            proxy,
            target,
            connect,
            Duration::from_secs(socks::HANDSHAKE_SECS),
            Some(token),
        )
        .map_err(DialError::Proxy),
        (None, Target::Addr(addr)) => {
            TcpStream::connect_timeout(addr, connect).map_err(|e| DialError::Connect(e.kind()))
        }
        (None, Target::Name(..)) => Err(DialError::NameWithoutProxy),
    }
}

/// Look up a seed name with the system's resolver.
pub fn resolve(name: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    match (name, port).to_socket_addrs() {
        Ok(found) => Ok(found.collect()),
        Err(e) => Err(e.to_string()),
    }
}

/// NR-72, NR-26: look a seed name up through the Tor proxy. The token keeps
/// the lookup on a circuit of its own (NR-78). The same time limits as a
/// connection through the proxy: the connection, then the handshake.
pub fn proxy_resolve(proxy: SocketAddr, name: &str, token: u64) -> Result<IpAddr, SocksError> {
    socks::resolve(
        proxy,
        name,
        token,
        Duration::from_secs(CONNECT_SECS.saturating_add(socks::HANDSHAKE_SECS)),
    )
}

type DialFn =
    dyn Fn(&Target, Option<SocketAddr>, u64) -> Result<TcpStream, DialError> + Send + Sync;
type ResolveFn = dyn Fn(&str, u16) -> Result<Vec<SocketAddr>, String> + Send + Sync;
type ProxyResolveFn = dyn Fn(SocketAddr, &str, u64) -> Result<IpAddr, SocksError> + Send + Sync;

/// How the driver reaches the outside. The real one is [`Net::system`]; the
/// tests pass others.
#[derive(Clone)]
pub(crate) struct Net {
    pub dial: Arc<DialFn>,
    pub resolve: Arc<ResolveFn>,
    /// Never called without a proxy; the system's resolver is never called
    /// with one (NR-26).
    pub proxy_resolve: Arc<ProxyResolveFn>,
}

impl Net {
    pub fn system() -> Self {
        Self {
            dial: Arc::new(dial),
            resolve: Arc::new(resolve),
            proxy_resolve: Arc::new(proxy_resolve),
        }
    }
}
