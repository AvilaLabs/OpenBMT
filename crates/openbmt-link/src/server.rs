//! A relay on a TCP listener (LK-30).
//!
//! One thread accepts, and one thread serves each connection. At most
//! [`MAX_CONNECTIONS`] connections are served at once; one more is closed at
//! once. A connection that sends no whole frame within the idle limit is
//! closed, and so is one that sends a bad frame or one over the limit. The
//! relay's state is behind one lock, so requests are answered one at a time.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use openbmt_net_io::Clock;

use crate::proto::{parse_header, Direction, Request, FRAME_HEADER_LEN};
use crate::random::os_random;
use crate::relay::Relay;
use crate::store::MailStore;

/// Connections served at once (LK-30).
pub const MAX_CONNECTIONS: usize = 32;
/// Seconds a connection may send nothing before it is closed (LK-30).
pub const IDLE_SECS: u64 = 30;
/// Seconds an answer may take to be written.
pub const WRITE_SECS: u64 = 10;

/// The limits of a server. The defaults are the ones in the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerConfig {
    /// Connections served at once.
    pub max_connections: usize,
    /// How long a connection may send nothing.
    pub idle: Duration,
    /// How long an answer may take to be written.
    pub write: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            idle: Duration::from_secs(IDLE_SECS),
            write: Duration::from_secs(WRITE_SECS),
        }
    }
}

/// A running server. Dropping it stops it.
pub struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

/// Counts a connection while it lives.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// How often a waiting thread looks at the stop flag.
const POLL: Duration = Duration::from_millis(50);

impl Server {
    /// Serve `relay` on a listener the caller bound.
    pub fn start<S>(
        listener: TcpListener,
        relay: Relay<S>,
        clock: Arc<dyn Clock + Send + Sync>,
        cfg: ServerConfig,
    ) -> io::Result<Self>
    where
        S: MailStore + Send + 'static,
    {
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let relay = Arc::new(Mutex::new(relay));
        let active = Arc::new(AtomicUsize::new(0));
        let accept = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("link-accept".into())
                .spawn(move || accept_loop(listener, relay, clock, cfg, stop, active))?
        };
        Ok(Self {
            addr,
            stop,
            accept: Some(accept),
        })
    }

    /// The address it listens on.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stop accepting and tell the connections to end.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop<S>(
    listener: TcpListener,
    relay: Arc<Mutex<Relay<S>>>,
    clock: Arc<dyn Clock + Send + Sync>,
    cfg: ServerConfig,
    stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
) where
    S: MailStore + Send + 'static,
{
    let mut next_conn: u64 = 0;
    while !stop.load(Ordering::SeqCst) {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        };
        if active.fetch_add(1, Ordering::SeqCst) >= cfg.max_connections {
            active.fetch_sub(1, Ordering::SeqCst);
            drop(stream);
            continue;
        }
        next_conn = next_conn.wrapping_add(1);
        let conn = next_conn;
        let slot = Slot(Arc::clone(&active));
        let relay = Arc::clone(&relay);
        let clock = Arc::clone(&clock);
        let stop = Arc::clone(&stop);
        let spawned = thread::Builder::new()
            .name("link-conn".into())
            .spawn(move || {
                let _slot = slot;
                serve(stream, conn, &relay, &*clock, cfg, &stop);
                // The connection is over: its challenge goes with it.
                relay
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .close(conn);
            });
        // If no thread could be made, the closure (and its slot) is gone and
        // the stream is closed.
        drop(spawned);
    }
}

/// Read exactly `buf.len()` bytes before `deadline`, looking at `stop`
/// between short waits.
fn read_full(
    stream: &mut TcpStream,
    buf: &mut [u8],
    deadline: Instant,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut filled = 0usize;
    while filled < buf.len() {
        if stop.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(Some(left.min(POLL)))?;
        let Some(rest) = buf.get_mut(filled..) else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        match stream.read(rest) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => filled = filled.saturating_add(n),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn serve<S: MailStore>(
    mut stream: TcpStream,
    conn: u64,
    relay: &Mutex<Relay<S>>,
    clock: &(dyn Clock + Send + Sync),
    cfg: ServerConfig,
    stop: &AtomicBool,
) {
    if stream.set_nonblocking(false).is_err() || stream.set_write_timeout(Some(cfg.write)).is_err()
    {
        return;
    }
    loop {
        let deadline = Instant::now() + cfg.idle;
        let mut header = [0u8; FRAME_HEADER_LEN];
        if read_full(&mut stream, &mut header, deadline, stop).is_err() {
            return;
        }
        let Ok((_, len)) = parse_header(&header, Direction::ToRelay) else {
            return;
        };
        let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + len);
        frame.extend_from_slice(&header);
        frame.resize(FRAME_HEADER_LEN + len, 0);
        let Some(body) = frame.get_mut(FRAME_HEADER_LEN..) else {
            return;
        };
        if read_full(&mut stream, body, deadline, stop).is_err() {
            return;
        }
        let Ok(request) = Request::decode(&frame) else {
            return;
        };
        let answer = {
            let mut relay = relay.lock().unwrap_or_else(PoisonError::into_inner);
            relay.handle(conn, request, clock.now(), &mut os_random)
        };
        let Ok(bytes) = answer.encode() else {
            return;
        };
        if stream.write_all(&bytes).is_err() {
            return;
        }
    }
}
