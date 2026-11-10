//! The client side of SOCKS5 (RFC 1928), for NR-26: with Tor on, every
//! connection goes through the proxy the caller names.
//!
//! Only the "no authentication" method is offered, and only CONNECT is
//! asked. A proxy that answers with any other method, any other version or a
//! reply that does not fit the form is an error with its own name. The
//! whole handshake has one deadline.

use std::fmt;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// NR-26: seconds the whole SOCKS5 handshake may take.
pub const HANDSHAKE_SECS: u64 = 20;

const VERSION: u8 = 5;
const METHOD_NONE: u8 = 0;
const METHOD_REFUSED: u8 = 0xff;
const CMD_CONNECT: u8 = 1;
const ATYP_V4: u8 = 1;
const ATYP_NAME: u8 = 3;
const ATYP_V6: u8 = 4;

/// Where the proxy is asked to connect to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    /// An IPv4 or IPv6 address and port.
    Addr(SocketAddr),
    /// A host name and port, which the proxy resolves. Never resolved here
    /// (NR-26).
    Name(String, u16),
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Addr(a) => write!(f, "{a}"),
            Self::Name(n, p) => write!(f, "{n}:{p}"),
        }
    }
}

/// The reply codes of RFC 1928 section 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyCode {
    /// 1: general SOCKS server failure.
    GeneralFailure,
    /// 2: connection not allowed by the rule set.
    NotAllowed,
    /// 3: network unreachable.
    NetworkUnreachable,
    /// 4: host unreachable.
    HostUnreachable,
    /// 5: connection refused.
    ConnectionRefused,
    /// 6: TTL expired.
    TtlExpired,
    /// 7: command not supported.
    CommandNotSupported,
    /// 8: address type not supported.
    AddressTypeNotSupported,
    /// Any other non-zero code.
    Other(u8),
}

impl ReplyCode {
    fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::GeneralFailure,
            2 => Self::NotAllowed,
            3 => Self::NetworkUnreachable,
            4 => Self::HostUnreachable,
            5 => Self::ConnectionRefused,
            6 => Self::TtlExpired,
            7 => Self::CommandNotSupported,
            8 => Self::AddressTypeNotSupported,
            other => Self::Other(other),
        }
    }
}

impl fmt::Display for ReplyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GeneralFailure => write!(f, "general failure"),
            Self::NotAllowed => write!(f, "connection not allowed"),
            Self::NetworkUnreachable => write!(f, "network unreachable"),
            Self::HostUnreachable => write!(f, "host unreachable"),
            Self::ConnectionRefused => write!(f, "connection refused"),
            Self::TtlExpired => write!(f, "TTL expired"),
            Self::CommandNotSupported => write!(f, "command not supported"),
            Self::AddressTypeNotSupported => write!(f, "address type not supported"),
            Self::Other(code) => write!(f, "reply code {code}"),
        }
    }
}

/// Why a SOCKS5 connection failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksError {
    /// The TCP connection to the proxy failed.
    Connect(ErrorKind),
    /// A read or write failed for a reason other than a timeout or a short
    /// answer.
    Io(ErrorKind),
    /// The handshake did not finish in time.
    TimedOut,
    /// The proxy closed the connection in the middle of an answer.
    Truncated,
    /// The proxy answered with a version other than 5.
    BadVersion(u8),
    /// The proxy said it accepts none of the methods offered (0xff).
    NoAcceptableMethod,
    /// The proxy chose a method that was not offered. The code is the one
    /// it chose.
    MethodNotOffered(u8),
    /// The proxy refused the connection with this reply code.
    Refused(ReplyCode),
    /// The reserved byte of the reply was not 0.
    BadReserved(u8),
    /// The reply carries an address type that is not 1, 3 or 4.
    BadAddressType(u8),
    /// The host name is empty.
    EmptyName,
    /// The host name is longer than 255 bytes.
    NameTooLong,
}

impl fmt::Display for SocksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(k) => write!(f, "NR-26: could not reach the proxy: {k}"),
            Self::Io(k) => write!(f, "NR-26: the proxy connection failed: {k}"),
            Self::TimedOut => write!(f, "NR-26: the proxy did not answer in time"),
            Self::Truncated => write!(f, "NR-26: the proxy's answer was cut short"),
            Self::BadVersion(v) => write!(f, "NR-26: the proxy answered with version {v}"),
            Self::NoAcceptableMethod => {
                write!(f, "NR-26: the proxy accepts no method without a password")
            }
            Self::MethodNotOffered(m) => {
                write!(
                    f,
                    "NR-26: the proxy chose method {m}, which was not offered"
                )
            }
            Self::Refused(code) => write!(f, "NR-26: the proxy refused: {code}"),
            Self::BadReserved(b) => write!(f, "NR-26: the reply has reserved byte {b}"),
            Self::BadAddressType(t) => write!(f, "NR-26: the reply has address type {t}"),
            Self::EmptyName => write!(f, "NR-26: the host name is empty"),
            Self::NameTooLong => write!(f, "NR-26: the host name is longer than 255 bytes"),
        }
    }
}

impl std::error::Error for SocksError {}

/// One deadline for every read and write of the handshake.
struct Deadline {
    end: Instant,
}

impl Deadline {
    fn left(&self) -> Result<Duration, SocksError> {
        let left = self.end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            Err(SocksError::TimedOut)
        } else {
            Ok(left)
        }
    }
}

fn io_error(e: &io::Error) -> SocksError {
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => SocksError::TimedOut,
        ErrorKind::UnexpectedEof => SocksError::Truncated,
        kind => SocksError::Io(kind),
    }
}

fn send(stream: &mut TcpStream, deadline: &Deadline, bytes: &[u8]) -> Result<(), SocksError> {
    stream
        .set_write_timeout(Some(deadline.left()?))
        .map_err(|e| io_error(&e))?;
    stream.write_all(bytes).map_err(|e| io_error(&e))
}

/// Read exactly `buf.len()` bytes before the deadline.
fn take(stream: &mut TcpStream, deadline: &Deadline, buf: &mut [u8]) -> Result<(), SocksError> {
    let mut filled = 0usize;
    while filled < buf.len() {
        stream
            .set_read_timeout(Some(deadline.left()?))
            .map_err(|e| io_error(&e))?;
        let Some(rest) = buf.get_mut(filled..) else {
            break;
        };
        match stream.read(rest) {
            Ok(0) => return Err(SocksError::Truncated),
            Ok(n) => filled = filled.saturating_add(n),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(io_error(&e)),
        }
    }
    Ok(())
}

/// The CONNECT request for a target.
fn request(target: &Target) -> Result<Vec<u8>, SocksError> {
    let mut out = vec![VERSION, CMD_CONNECT, 0];
    let port = match target {
        Target::Addr(addr) => {
            match addr.ip() {
                IpAddr::V4(ip) => {
                    out.push(ATYP_V4);
                    out.extend_from_slice(&ip.octets());
                }
                IpAddr::V6(ip) => {
                    out.push(ATYP_V6);
                    out.extend_from_slice(&ip.octets());
                }
            }
            addr.port()
        }
        Target::Name(name, port) => {
            let bytes = name.as_bytes();
            if bytes.is_empty() {
                return Err(SocksError::EmptyName);
            }
            let len = u8::try_from(bytes.len()).map_err(|_| SocksError::NameTooLong)?;
            out.push(ATYP_NAME);
            out.push(len);
            out.extend_from_slice(bytes);
            *port
        }
    };
    out.extend_from_slice(&port.to_be_bytes());
    Ok(out)
}

/// Run the handshake on a stream that is connected to the proxy, and ask it
/// to connect to `target`. The whole handshake must end within `total`.
///
/// On success the stream is a tunnel to the target; its timeouts are
/// cleared. A reply with a non-zero code is read no further than its first
/// four bytes. The address the proxy says it bound to is read and ignored.
pub fn handshake(
    stream: &mut TcpStream,
    target: &Target,
    total: Duration,
) -> Result<(), SocksError> {
    let deadline = Deadline {
        end: Instant::now() + total,
    };
    let request = request(target)?;
    // Offer one method: no authentication.
    send(stream, &deadline, &[VERSION, 1, METHOD_NONE])?;
    let mut choice = [0u8; 2];
    take(stream, &deadline, &mut choice)?;
    let [version, method] = choice;
    if version != VERSION {
        return Err(SocksError::BadVersion(version));
    }
    match method {
        METHOD_NONE => {}
        METHOD_REFUSED => return Err(SocksError::NoAcceptableMethod),
        other => return Err(SocksError::MethodNotOffered(other)),
    }
    send(stream, &deadline, &request)?;
    let mut head = [0u8; 4];
    take(stream, &deadline, &mut head)?;
    let [version, reply, reserved, atyp] = head;
    if version != VERSION {
        return Err(SocksError::BadVersion(version));
    }
    if reply != 0 {
        return Err(SocksError::Refused(ReplyCode::from_byte(reply)));
    }
    if reserved != 0 {
        return Err(SocksError::BadReserved(reserved));
    }
    // The bound address and port: read, not used.
    let bound = match atyp {
        ATYP_V4 => 4usize,
        ATYP_V6 => 16,
        ATYP_NAME => {
            let mut len = [0u8; 1];
            take(stream, &deadline, &mut len)?;
            usize::from(u8::from_be_bytes(len))
        }
        other => return Err(SocksError::BadAddressType(other)),
    };
    let mut rest = vec![0u8; bound.saturating_add(2)];
    take(stream, &deadline, &mut rest)?;
    stream.set_read_timeout(None).map_err(|e| io_error(&e))?;
    stream.set_write_timeout(None).map_err(|e| io_error(&e))?;
    Ok(())
}

/// Connect to the proxy (`connect_timeout` for the TCP connection) and ask
/// it to connect to `target`, all within `total` for the handshake.
pub fn connect_through(
    proxy: SocketAddr,
    target: &Target,
    connect_timeout: Duration,
    total: Duration,
) -> Result<TcpStream, SocksError> {
    let mut stream = TcpStream::connect_timeout(&proxy, connect_timeout)
        .map_err(|e| SocksError::Connect(e.kind()))?;
    handshake(&mut stream, target, total)?;
    Ok(stream)
}
