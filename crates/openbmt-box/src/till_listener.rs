//! The till listener (BX-70 to BX-75): a small JSON interface on its own
//! listener, off unless the Box is given an address for it.
//!
//! The listener is plain HTTP/1.1 written with the standard library alone,
//! like the status page and with the same kind of bounds, but it is its own
//! listener on its own threads and it answers JSON only. It takes a sale
//! request from a till, says how the sale stands, and cancels it. Every
//! request carries a bearer token made from the till key and a nonce, and
//! every answer to one with a key carries a mac under the reply key (BX-71,
//! BX-80).
//!
//! [`parse_request`] and [`parse_object`] are the readers of the bytes of one
//! request and of its JSON body; they are what the fuzz target calls.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use openbmt_core::say::{clean_foreign, Words};

use crate::say::{TillAnswer, TILL_PAID};
use crate::status_page::{head_end, head_is_clean, is_token_byte, response, Slot};
use crate::till::{
    hex, reply_mac, request_id_ok, unhex, KeyCheck, SaleInfo, SaleState, TillRefusal,
};

/// Most bytes of a whole request, head and body together (BX-70).
pub const MAX_REQUEST: usize = 4 * 1024;
/// Connections served at once; one more is closed at once (BX-70).
pub const MAX_CONNECTIONS: usize = 16;
/// Seconds a whole request may take to arrive (BX-70).
pub const READ_SECS: u64 = 5;
/// Seconds the answer may take to be written (BX-70).
pub const WRITE_SECS: u64 = 5;
/// The most headers a request may have.
const MAX_HEADERS: usize = 32;
/// How often a waiting thread looks at the stop flag.
const POLL: Duration = Duration::from_millis(50);

/// The limits of the listener. The defaults are the ones in the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TillConfig {
    /// Connections served at once.
    pub max_connections: usize,
    /// How long a whole request may take to arrive.
    pub read: Duration,
    /// How long the answer may take to be written.
    pub write: Duration,
}

impl Default for TillConfig {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            read: Duration::from_secs(READ_SECS),
            write: Duration::from_secs(WRITE_SECS),
        }
    }
}

/// The three routes there are (BX-72 to BX-74). The sale id of the other two
/// is `None` when the text in the path is not 32 lowercase hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `POST /till/v1/sales`.
    Create,
    /// `GET /till/v1/sales/<sale>`.
    Read(Option<[u8; 16]>),
    /// `POST /till/v1/sales/<sale>/cancel`.
    Cancel(Option<[u8; 16]>),
}

/// A request that was read whole and passed the checks of BX-70.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Which route.
    pub route: Route,
    /// The method, as sent.
    pub method: String,
    /// The path, as sent.
    pub path: String,
    /// The nonce of the request: 32 lowercase hex (BX-80).
    pub nonce: String,
    /// The bearer token, if the request carried one.
    pub key: Option<String>,
    /// The body (empty unless the route is `Create`).
    pub body: Vec<u8>,
}

/// Why a request was set aside before it reached the till's routes. The
/// answer holds no words (BX-75).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// 400: not a request this listener reads.
    BadRequest,
    /// 404: no such path, or not this method on it.
    NotFound,
    /// 413: over the limit.
    TooLarge,
}

impl Reject {
    /// The status code.
    pub fn status(self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::NotFound => 404,
            Self::TooLarge => 413,
        }
    }

    /// The key the body carries.
    fn key(self) -> &'static str {
        match self {
            Self::BadRequest => "tillBadRequest",
            Self::NotFound => "tillNotFound",
            Self::TooLarge => "tillTooLarge",
        }
    }
}

/// The route a method and a target name, if any. A path that is not one of
/// the three, and a method that is not the right one for a path, are both
/// "not found" (BX-75).
fn route_of(method: &str, target: &str) -> Option<Route> {
    if target == "/till/v1/sales" {
        return (method == "POST").then_some(Route::Create);
    }
    let rest = target.strip_prefix("/till/v1/sales/")?;
    match rest.split_once('/') {
        None if method == "GET" => Some(Route::Read(unhex::<16>(rest))),
        Some((sale, "cancel")) if method == "POST" => Some(Route::Cancel(unhex::<16>(sale))),
        _ => None,
    }
}

struct Head {
    route: Route,
    method: String,
    path: String,
    nonce: String,
    key: Option<String>,
    content_length: usize,
}

fn parse_head(head: &[u8], head_len: usize) -> Result<Head, Reject> {
    if !head_is_clean(head) {
        return Err(Reject::BadRequest);
    }
    let text = std::str::from_utf8(head).map_err(|_| Reject::BadRequest)?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().ok_or(Reject::BadRequest)?;
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Reject::BadRequest);
    };
    if method.is_empty() || !method.bytes().all(is_token_byte) {
        return Err(Reject::BadRequest);
    }
    if version != "HTTP/1.1" || !target.starts_with('/') {
        return Err(Reject::BadRequest);
    }
    let route = route_of(method, target).ok_or(Reject::NotFound)?;

    let mut key = None;
    let mut nonce: Option<String> = None;
    let mut content_length = None;
    let mut json = false;
    let mut count = 0usize;
    for line in lines {
        count = count.saturating_add(1);
        if count > MAX_HEADERS || line.starts_with([' ', '\t']) {
            return Err(Reject::BadRequest);
        }
        let (name, value) = line.split_once(':').ok_or(Reject::BadRequest)?;
        if name.is_empty() || !name.bytes().all(is_token_byte) {
            return Err(Reject::BadRequest);
        }
        let value = value.trim_matches([' ', '\t']);
        match name.to_ascii_lowercase().as_str() {
            "authorization" => {
                if key.is_some() {
                    return Err(Reject::BadRequest);
                }
                // A scheme other than Bearer is a key that is not the
                // Box's, which the answer says; the token is read as
                // nothing.
                let token = match value.split_once(' ') {
                    Some((scheme, token)) if scheme.eq_ignore_ascii_case("bearer") => {
                        token.trim_matches([' ', '\t'])
                    }
                    _ => "",
                };
                key = Some(token.to_string());
            }
            "x-obm-nonce" => {
                if nonce.is_some() || unhex::<16>(value).is_none() {
                    return Err(Reject::BadRequest);
                }
                nonce = Some(value.to_string());
            }
            "content-length" => {
                if content_length.is_some()
                    || value.is_empty()
                    || value.len() > 6
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(Reject::BadRequest);
                }
                content_length = Some(value.parse::<usize>().map_err(|_| Reject::BadRequest)?);
            }
            "content-type" => {
                let media = value.split(';').next().unwrap_or("").trim();
                json = media.eq_ignore_ascii_case("application/json");
            }
            "transfer-encoding" | "content-encoding" => return Err(Reject::BadRequest),
            _ => {}
        }
    }
    let content_length = match (route, content_length) {
        (Route::Create, Some(n)) if n > 0 => {
            if head_len.saturating_add(n) > MAX_REQUEST {
                return Err(Reject::TooLarge);
            }
            if !json {
                return Err(Reject::BadRequest);
            }
            n
        }
        (Route::Create, _) => return Err(Reject::BadRequest),
        (_, None | Some(0)) => 0,
        (_, Some(_)) => return Err(Reject::BadRequest),
    };
    // The nonce is looked for after the path and before the key (BX-75,
    // BX-80).
    let nonce = nonce.ok_or(Reject::BadRequest)?;
    Ok(Head {
        route,
        method: method.to_string(),
        path: target.to_string(),
        nonce,
        key,
        content_length,
    })
}

/// Read one request from the bytes received so far (BX-70). `Ok(None)` means
/// the bytes are a good start and more are needed; a rejection is final.
/// Bytes after the body are not looked at: one request is served for each
/// connection.
pub fn parse_request(buf: &[u8]) -> Result<Option<Request>, Reject> {
    let Some(end) = head_end(buf) else {
        return if buf.len() > MAX_REQUEST {
            Err(Reject::TooLarge)
        } else {
            Ok(None)
        };
    };
    let body_at = end.saturating_add(4);
    if body_at > MAX_REQUEST {
        return Err(Reject::TooLarge);
    }
    let head = buf.get(..end).ok_or(Reject::BadRequest)?;
    let head = parse_head(head, body_at)?;
    let body_end = body_at.saturating_add(head.content_length);
    if buf.len() < body_end {
        return Ok(None);
    }
    let body = buf.get(body_at..body_end).ok_or(Reject::BadRequest)?;
    Ok(Some(Request {
        route: head.route,
        method: head.method,
        path: head.path,
        nonce: head.nonce,
        key: head.key,
        body: body.to_vec(),
    }))
}

// ---- the JSON body ----------------------------------------------------

struct Cursor<'a> {
    text: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.at += 1;
        Some(b)
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, b: u8) -> Option<()> {
        (self.bump()? == b).then_some(())
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            let d = char::from(self.bump()?).to_digit(16)?;
            v = v * 16 + d;
        }
        Some(v)
    }

    /// A JSON string, after its opening quote has been read.
    fn string(&mut self) -> Option<String> {
        let mut out: Vec<u8> = Vec::new();
        loop {
            match self.bump()? {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let c = match self.bump()? {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            if (0xd800..0xdc00).contains(&hi) {
                                self.expect(b'\\')?;
                                self.expect(b'u')?;
                                let lo = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&lo) {
                                    return None;
                                }
                                char::from_u32(0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00))?
                            } else {
                                char::from_u32(hi)?
                            }
                        }
                        _ => return None,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                // A raw control character is not allowed inside a string.
                b if b < 0x20 => return None,
                b => out.push(b),
            }
        }
    }
}

/// Read a JSON object whose every value is a string, as the till's request
/// body is (BX-72). `None` for anything else: another kind of value, a key
/// twice, bytes after the object, text that is not UTF-8.
pub fn parse_object(body: &[u8]) -> Option<BTreeMap<String, String>> {
    let mut c = Cursor { text: body, at: 0 };
    let mut out = BTreeMap::new();
    c.space();
    c.expect(b'{')?;
    c.space();
    if c.peek() == Some(b'}') {
        c.at += 1;
    } else {
        loop {
            c.space();
            c.expect(b'"')?;
            let key = c.string()?;
            c.space();
            c.expect(b':')?;
            c.space();
            c.expect(b'"')?;
            let value = c.string()?;
            if out.insert(key, value).is_some() {
                return None;
            }
            c.space();
            match c.bump()? {
                b',' => {}
                b'}' => break,
                _ => return None,
            }
        }
    }
    c.space();
    (c.at == body.len()).then_some(out)
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn words_json(heading: &str, what: &str, next: &str) -> String {
    format!(
        "{{\"heading\":{},\"what\":{},\"next\":{}}}",
        json_string(heading),
        json_string(what),
        json_string(next)
    )
}

// ---- the answers ------------------------------------------------------

/// What the sale the till asked for is (BX-72).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateBody {
    /// The counter's code as the till gave it.
    pub counter: String,
    /// The amount as the till gave it.
    pub amount: String,
    /// The currency as the till gave it.
    pub currency: String,
    /// The reference as the till gave it.
    pub reference: String,
    /// The till's own id for the request.
    pub request: String,
}

/// The Box as the till listener sees it. Each call takes the state's locks
/// only for itself, after a whole request was read (BX-60 applies here too).
pub trait TillBackend: Send + Sync + 'static {
    /// What the bearer token the till presented comes to against the till
    /// key file (BX-71, BX-80).
    fn check_key(&self, presented: Option<&str>) -> KeyCheck;
    /// BX-72: ask a counter for a sale.
    fn create(&self, body: CreateBody) -> Result<([u8; 16], SaleInfo), TillRefusal>;
    /// BX-73: how a sale stands.
    fn read(&self, sale: &[u8; 16]) -> Result<SaleInfo, TillRefusal>;
    /// BX-74: cancel a sale.
    fn cancel(&self, sale: &[u8; 16]) -> Result<SaleInfo, TillRefusal>;
}

impl TillRefusal {
    /// The words the till is given, if the refusal has any.
    pub fn answer(&self) -> Option<TillAnswer> {
        Some(match self {
            Self::KeyWrong => TillAnswer::KeyWrong,
            Self::KeyOld => TillAnswer::KeyOld,
            Self::NotPaired => TillAnswer::NotPaired,
            Self::UnknownCounter(code) => {
                TillAnswer::UnknownCounter(clean_foreign(code).chars().take(16).collect())
            }
            Self::WrongMoney { shop, asked } => TillAnswer::WrongMoney {
                shop: shop.clone(),
                asked: clean_foreign(asked).chars().take(8).collect(),
            },
            Self::BadAmount => TillAnswer::BadAmount,
            Self::Busy => TillAnswer::Busy,
            Self::Lost => TillAnswer::Lost,
            Self::BadRequest | Self::Fault => return None,
        })
    }

    fn key(&self) -> &'static str {
        match self.answer() {
            Some(answer) => answer.key(),
            None if *self == Self::Fault => "tillFault",
            None => "tillBadRequest",
        }
    }
}

fn json(status: u16, body: &str, mac: Option<&str>) -> Vec<u8> {
    let extra = mac.map_or(String::new(), |mac| format!("X-OBM-Reply: {mac}\r\n"));
    response(status, "application/json", &extra, body)
}

/// The answer to a refused request: the status and a fixed body with no
/// words (BX-75). The request had no nonce or no key to sign under, so the
/// answer carries no mac (BX-80).
pub fn reject_response(reject: Reject) -> Vec<u8> {
    json(
        reject.status(),
        &format!("{{\"error\":\"{}\"}}", reject.key()),
        None,
    )
}

/// The status and body of a refusal of the till's request: the key and,
/// when there are any, the words (BX-75).
fn refusal_parts(refusal: &TillRefusal) -> (u16, String) {
    let body = match refusal.answer() {
        Some(answer) => {
            let w: Words = answer.words();
            format!(
                "{{\"error\":\"{}\",\"words\":{}}}",
                refusal.key(),
                words_json(&w.heading, &w.what, &w.next)
            )
        }
        None => format!("{{\"error\":\"{}\"}}", refusal.key()),
    };
    (refusal.status(), body)
}

/// The answer to a refusal of the till's request, without a mac.
pub fn refusal_response(refusal: &TillRefusal) -> Vec<u8> {
    let (status, body) = refusal_parts(refusal);
    json(status, &body, None)
}

/// The answer to `request`: `status` and `body`, with the mac of BX-80 when
/// there is a reply key to sign under.
fn signed(request: &Request, reply_key: Option<&[u8; 32]>, status: u16, body: &str) -> Vec<u8> {
    let mac = reply_key.map(|key| {
        reply_mac(
            key,
            &request.method,
            &request.path,
            &request.nonce,
            &request.body,
            status,
            body.as_bytes(),
        )
    });
    json(status, body, mac.as_deref())
}

/// The start of a sale's body (BX-72, BX-73): the sale, the state and the
/// fields as the Box took them, without the closing brace.
fn sale_head(sale: &[u8; 16], info: &SaleInfo) -> String {
    format!(
        "{{\"sale\":\"{}\",\"state\":\"{}\",\"amount\":{},\"currency\":{},\"request\":{},\"reference\":{}",
        hex(sale),
        info.state.word(),
        json_string(&info.amount),
        json_string(&info.currency),
        json_string(&info.request),
        json_string(&info.reference)
    )
}

/// The body that says how a sale stands (BX-73): the sale, the state, the
/// fields the Box took, and for a sale that has ended the words.
pub fn sale_body(sale: &[u8; 16], info: &SaleInfo) -> String {
    let words = match info.state {
        SaleState::Waiting | SaleState::Opened => None,
        // The one plain line of the till's own, as it is shown.
        SaleState::Paid => Some(words_json("", TILL_PAID, "")),
        SaleState::Unpaid => Some(answer_json(&TillAnswer::Unpaid)),
        SaleState::Expired => Some(answer_json(&TillAnswer::Expired)),
        SaleState::Cancelled => Some(answer_json(&TillAnswer::Cancelled)),
    };
    let head = sale_head(sale, info);
    match words {
        Some(words) => format!("{head},\"words\":{words}}}"),
        None => format!("{head}}}"),
    }
}

fn answer_json(answer: &TillAnswer) -> String {
    let w = answer.words();
    words_json(&w.heading, &w.what, &w.next)
}

fn create_body(fields: &BTreeMap<String, String>) -> Option<CreateBody> {
    let get = |name: &str| fields.get(name).cloned();
    let body = CreateBody {
        counter: get("counter")?,
        amount: get("amount")?,
        currency: get("currency")?,
        reference: get("reference").unwrap_or_default(),
        request: get("request")?,
    };
    request_id_ok(&body.request).then_some(body)
}

/// The answer to a request that passed the reader. Every answer after the
/// nonce carries the mac of BX-80 when the Box holds a reply key.
pub fn answer(backend: &dyn TillBackend, request: &Request) -> Vec<u8> {
    let reply_key = match backend.check_key(request.key.as_deref()) {
        KeyCheck::Good(reply) => reply,
        KeyCheck::Wrong(reply) => {
            let (status, body) = refusal_parts(&TillRefusal::KeyWrong);
            return signed(request, reply.as_ref(), status, &body);
        }
        KeyCheck::Old => {
            let (status, body) = refusal_parts(&TillRefusal::KeyOld);
            return signed(request, None, status, &body);
        }
    };
    let result = match request.route {
        Route::Create => match parse_object(&request.body).as_ref().and_then(create_body) {
            Some(body) => backend.create(body),
            None => Err(TillRefusal::BadRequest),
        },
        Route::Read(Some(id)) => backend.read(&id).map(|info| (id, info)),
        Route::Cancel(Some(id)) => backend.cancel(&id).map(|info| (id, info)),
        Route::Read(None) | Route::Cancel(None) => Err(TillRefusal::Lost),
    };
    match result {
        Ok((id, info)) => {
            let body = match request.route {
                // A new sale is told with no words.
                Route::Create => format!("{}}}", sale_head(&id, &info)),
                _ => sale_body(&id, &info),
            };
            signed(request, Some(&reply_key), 200, &body)
        }
        Err(refusal) => {
            let (status, body) = refusal_parts(&refusal);
            signed(request, Some(&reply_key), status, &body)
        }
    }
}

// ---- the listener -----------------------------------------------------

/// A running till listener. Dropping it stops it.
pub struct TillListener {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl TillListener {
    /// Serve `backend` on a listener the caller bound.
    pub fn start(
        listener: TcpListener,
        backend: Arc<dyn TillBackend>,
        cfg: TillConfig,
    ) -> io::Result<Self> {
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        let accept = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("till-accept".into())
                .spawn(move || accept_loop(listener, backend, cfg, stop, active))?
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

impl Drop for TillListener {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(
    listener: TcpListener,
    backend: Arc<dyn TillBackend>,
    cfg: TillConfig,
    stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
) {
    let mut backoff = Duration::from_millis(10);
    while !stop.load(Ordering::SeqCst) {
        let stream = match listener.accept() {
            Ok((stream, _)) => {
                backoff = Duration::from_millis(10);
                stream
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(_) => {
                thread::sleep(backoff);
                backoff = backoff.saturating_mul(2).min(Duration::from_secs(1));
                continue;
            }
        };
        if active.fetch_add(1, Ordering::SeqCst) >= cfg.max_connections {
            active.fetch_sub(1, Ordering::SeqCst);
            drop(stream);
            continue;
        }
        let slot = Slot(Arc::clone(&active));
        let backend = Arc::clone(&backend);
        let stop = Arc::clone(&stop);
        // If no thread could be made, the closure (and its slot) is gone and
        // the stream is closed.
        let spawned = thread::Builder::new()
            .name("till-conn".into())
            .spawn(move || {
                let _slot = slot;
                serve(stream, &*backend, cfg, &stop);
            });
        drop(spawned);
    }
}

/// Read one request before the deadline, looking at `stop` between short
/// waits.
fn read_request(
    stream: &mut TcpStream,
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<Request, Reject> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(request) = parse_request(&buf)? {
            return Ok(request);
        }
        if stop.load(Ordering::SeqCst) {
            return Err(Reject::BadRequest);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left.min(POLL))).is_err() {
            return Err(Reject::BadRequest);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err(Reject::BadRequest),
            Ok(n) => buf.extend_from_slice(chunk.get(..n).unwrap_or(&[])),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return Err(Reject::BadRequest),
        }
    }
}

fn serve(mut stream: TcpStream, backend: &dyn TillBackend, cfg: TillConfig, stop: &AtomicBool) {
    if stream.set_nonblocking(false).is_err() || stream.set_write_timeout(Some(cfg.write)).is_err()
    {
        return;
    }
    let bytes = match read_request(&mut stream, Instant::now() + cfg.read, stop) {
        Ok(request) => answer(backend, &request),
        // A connection that went quiet or closed gets no answer.
        Err(Reject::BadRequest) if stop.load(Ordering::SeqCst) => return,
        Err(reject) => reject_response(reject),
    };
    if stream.write_all(&bytes).is_ok() {
        let _ = stream.flush();
    }
    let _ = stream.shutdown(Shutdown::Write);
}
