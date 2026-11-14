//! The Box's status page (BX-54 to BX-61): a small web page on its own
//! listener, off unless the Box is given an address for it.
//!
//! The page is plain HTTP/1.1 written by hand with the standard library
//! alone. It answers `GET /` with the Box's state in words and, while the Box
//! waits for an invitation, `POST /invitation` with the text of one. It runs
//! on its own threads and takes the Box's state only for the moment it reads
//! or enters something, after a whole request has arrived, so a slow or
//! hostile connection can only use up the page's own connection slots.
//!
//! [`parse_request`] is the request reader: it works on the bytes of one
//! request and is what the fuzz target calls. The socket code reads until
//! the bytes are whole and hands them to it.

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use openbmt_core::say::{Tone, Words};

use crate::say::{ConsoleAnswer, PageAnswer};
use crate::state::{BoxError, BoxStatus};

/// Most bytes of the request line and the headers (BX-54).
pub const MAX_HEAD: usize = 8 * 1024;
/// Most bytes of a request's body (BX-54).
pub const MAX_BODY: usize = 1024;
/// Connections the page serves at once; one more is closed at once (BX-54).
pub const MAX_CONNECTIONS: usize = 16;
/// Seconds a whole request may take to arrive (BX-54).
pub const READ_SECS: u64 = 5;
/// Seconds the answer may take to be written.
pub const WRITE_SECS: u64 = 5;
/// The name of the form field that holds the invitation (BX-57).
pub const FIELD: &str = "invitation";
/// The most headers a request may have.
const MAX_HEADERS: usize = 64;
/// How often a waiting thread looks at the stop flag.
const POLL: Duration = Duration::from_millis(50);

/// The limits of a page. The defaults are the ones in the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageConfig {
    /// Connections served at once.
    pub max_connections: usize,
    /// How long a whole request may take to arrive.
    pub read: Duration,
    /// How long the answer may take to be written.
    pub write: Duration,
}

impl Default for PageConfig {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            read: Duration::from_secs(READ_SECS),
            write: Duration::from_secs(WRITE_SECS),
        }
    }
}

/// The two pages there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `GET /`.
    Home,
    /// `POST /invitation`.
    Invitation,
}

/// A request that was read whole and passed every check of BX-54.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Which page.
    pub route: Route,
    /// The `Host` header, as sent.
    pub host: String,
    /// The `Origin` header, if sent.
    pub origin: Option<String>,
    /// The `Referer` header, if sent.
    pub referer: Option<String>,
    /// The body (empty for `GET`).
    pub body: Vec<u8>,
}

/// Why a request was refused. The answer says no more than the status
/// (BX-54).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 400: not a request this page reads.
    BadRequest,
    /// 404: no such page.
    NotFound,
    /// 405: the page exists but not for this method.
    MethodNotAllowed(Route),
    /// 413: the head or the body is over its limit.
    TooLarge,
}

impl Refusal {
    /// The status code.
    pub fn status(self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::NotFound => 404,
            Self::MethodNotAllowed(_) => 405,
            Self::TooLarge => 413,
        }
    }
}

/// The head of a request, read.
struct Head {
    route: Route,
    host: String,
    origin: Option<String>,
    referer: Option<String>,
    content_length: usize,
}

/// Where the head ends: the index of the blank line's first byte.
pub(crate) fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

pub(crate) fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// Every byte of a head is printable ASCII or a tab, and every line ends in
/// CR LF with no CR or LF alone.
pub(crate) fn head_is_clean(head: &[u8]) -> bool {
    let mut lines = head.split(|&b| b == b'\n').peekable();
    while let Some(line) = lines.next() {
        // Each line but the last ends with the CR before its LF; the last
        // has none.
        let body = if lines.peek().is_some() {
            match line.split_last() {
                Some((b'\r', rest)) => rest,
                _ => return false,
            }
        } else {
            line
        };
        if body
            .iter()
            .any(|&b| !(b == b'\t' || (0x20..0x7f).contains(&b)))
        {
            return false;
        }
    }
    true
}

fn parse_head(head: &[u8]) -> Result<Head, Refusal> {
    if !head_is_clean(head) {
        return Err(Refusal::BadRequest);
    }
    let text = std::str::from_utf8(head).map_err(|_| Refusal::BadRequest)?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().ok_or(Refusal::BadRequest)?;
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refusal::BadRequest);
    };
    if method.is_empty() || !method.bytes().all(is_token_byte) {
        return Err(Refusal::BadRequest);
    }
    if version != "HTTP/1.1" || !target.starts_with('/') {
        return Err(Refusal::BadRequest);
    }
    let (route, allowed) = match target {
        "/" => (Route::Home, "GET"),
        "/invitation" => (Route::Invitation, "POST"),
        _ => return Err(Refusal::NotFound),
    };
    if method != allowed {
        return Err(Refusal::MethodNotAllowed(route));
    }

    let mut host = None;
    let mut origin = None;
    let mut referer = None;
    let mut content_length = None;
    let mut content_type_ok = false;
    let mut count = 0usize;
    for line in lines {
        count = count.saturating_add(1);
        if count > MAX_HEADERS {
            return Err(Refusal::BadRequest);
        }
        // No folded lines: a line may not start with white space.
        if line.starts_with([' ', '\t']) {
            return Err(Refusal::BadRequest);
        }
        let (name, value) = line.split_once(':').ok_or(Refusal::BadRequest)?;
        if name.is_empty() || !name.bytes().all(is_token_byte) {
            return Err(Refusal::BadRequest);
        }
        let value = value.trim_matches([' ', '\t']);
        let once = |slot: &mut Option<String>| {
            if slot.is_some() {
                return Err(Refusal::BadRequest);
            }
            *slot = Some(value.to_string());
            Ok(())
        };
        match name.to_ascii_lowercase().as_str() {
            "host" => once(&mut host)?,
            "origin" => once(&mut origin)?,
            "referer" => once(&mut referer)?,
            "content-length" => {
                if content_length.is_some()
                    || value.is_empty()
                    || value.len() > 6
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(Refusal::BadRequest);
                }
                content_length = Some(value.parse::<usize>().map_err(|_| Refusal::BadRequest)?);
            }
            "content-type" => {
                let media = value.split(';').next().unwrap_or("").trim();
                content_type_ok = media.eq_ignore_ascii_case("application/x-www-form-urlencoded");
            }
            // A body of any other framing is not read.
            "transfer-encoding" | "content-encoding" => return Err(Refusal::BadRequest),
            _ => {}
        }
    }
    let host = host.ok_or(Refusal::BadRequest)?;
    if host.is_empty() || host.len() > 255 {
        return Err(Refusal::BadRequest);
    }
    let content_length = match (route, content_length) {
        (Route::Home, None | Some(0)) => 0,
        (Route::Home, Some(_)) => return Err(Refusal::BadRequest),
        (Route::Invitation, Some(n)) => {
            if n > MAX_BODY {
                return Err(Refusal::TooLarge);
            }
            if !content_type_ok {
                return Err(Refusal::BadRequest);
            }
            n
        }
        (Route::Invitation, None) => return Err(Refusal::BadRequest),
    };
    Ok(Head {
        route,
        host,
        origin,
        referer,
        content_length,
    })
}

/// The head of a request and where its body starts, or `None` while the
/// head is not whole.
fn split_head(buf: &[u8]) -> Result<Option<(Head, usize)>, Refusal> {
    match head_end(buf) {
        Some(end) if end > MAX_HEAD => Err(Refusal::TooLarge),
        Some(end) => {
            let head = buf.get(..end).ok_or(Refusal::BadRequest)?;
            Ok(Some((parse_head(head)?, end.saturating_add(4))))
        }
        None if buf.len() > MAX_HEAD.saturating_add(3) => Err(Refusal::TooLarge),
        None => Ok(None),
    }
}

/// Read one request from the bytes received so far (BX-54). `Ok(None)`
/// means the bytes are a good start and more are needed; a refusal is final.
/// Bytes after the body are not looked at: one request is served for each
/// connection.
pub fn parse_request(buf: &[u8]) -> Result<Option<Request>, Refusal> {
    let Some((head, body_at)) = split_head(buf)? else {
        return Ok(None);
    };
    let body_end = body_at.saturating_add(head.content_length);
    if buf.len() < body_end {
        return Ok(None);
    }
    let body = buf.get(body_at..body_end).ok_or(Refusal::BadRequest)?;
    Ok(Some(Request {
        route: head.route,
        host: head.host,
        origin: head.origin,
        referer: head.referer,
        body: body.to_vec(),
    }))
}

/// The authority (`host:port`) of an origin or a referer URL, if it is an
/// `http` or `https` URL with no user information. An origin has nothing
/// after the authority.
fn authority(url: &str, origin: bool) -> Option<&str> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .or_else(|| url.strip_prefix("HTTP://"))
        .or_else(|| url.strip_prefix("HTTPS://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (auth, after) = rest.split_at(end);
    if auth.is_empty() || auth.contains('@') || (origin && !after.is_empty()) {
        return None;
    }
    Some(auth)
}

/// BX-58: the request came from this page. The `Origin` header (or, with
/// none, the `Referer`) must name the same host and port as `Host`; with
/// neither the request is refused.
pub fn same_origin(host: &str, origin: Option<&str>, referer: Option<&str>) -> bool {
    let auth = match (origin, referer) {
        (Some(origin), _) => authority(origin, true),
        (None, Some(referer)) => authority(referer, false),
        (None, None) => None,
    };
    auth.is_some_and(|a| a.eq_ignore_ascii_case(host))
}

/// A form body that is not well formed, has the field twice, or does not
/// decode to text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadForm;

/// The value of the form field `name` in an urlencoded body.
pub fn form_field(body: &[u8], name: &str) -> Result<Option<String>, BadForm> {
    let mut found = None;
    for pair in body.split(|&b| b == b'&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.iter().position(|&b| b == b'=') {
            Some(at) => (
                pair.get(..at).unwrap_or(&[]),
                pair.get(at + 1..).unwrap_or(&[]),
            ),
            None => (pair, &[][..]),
        };
        if decode_component(key)? == name {
            if found.is_some() {
                return Err(BadForm);
            }
            found = Some(decode_component(value)?);
        }
    }
    Ok(found)
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn decode_component(raw: &[u8]) -> Result<String, BadForm> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0usize;
    while let Some(&b) = raw.get(i) {
        match b {
            b'+' => out.push(b' '),
            b'%' => {
                let hi = raw.get(i + 1).copied().and_then(hex_value).ok_or(BadForm)?;
                let lo = raw.get(i + 2).copied().and_then(hex_value).ok_or(BadForm)?;
                out.push(hi * 16 + lo);
                i += 2;
            }
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8(out).map_err(|_| BadForm)
}

/// What the page shows of the Box (BX-55): words and counts, never a key, an
/// invitation, a device's address or a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageView {
    /// What the Box is doing.
    pub status: BoxStatus,
    /// The program's version.
    pub version: &'static str,
    /// The port the relay listens on.
    pub relay_port: u16,
    /// Devices on the list the relay serves (BX-22).
    pub devices: usize,
    /// The shop's four short words, once paired.
    pub shop: Option<[&'static str; 4]>,
    /// The Box's last note, in words.
    pub last_note: Option<Words>,
    /// The store of Shop Lightning's state is full (BX-92).
    pub store_full: bool,
}

/// The words of a full store (BX-92).
pub fn store_full_words() -> Words {
    Words::new(
        Tone::Stop,
        "Lightning store is full",
        "The Box holds as much of Shop Lightning's state as it may, so the Lightning counter offers no Lightning payments now.",
        openbmt_core::say::Who::BoxKeeper,
        "Tell the shop's owner. Turning Shop Lightning off on the owner phone closes its channels and sends the money to the shop's wallet.",
    )
    .code("BX-92")
}

/// What came of a posted invitation (BX-57).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entered {
    /// The Box took it and offers its claim.
    Taken,
    /// The Box already holds an invitation that has not run out.
    Held,
    /// The Box refused it.
    Refused(BoxError),
}

/// The Box as the page sees it.
pub trait PageBackend: Send + Sync + 'static {
    /// The state now.
    fn view(&self) -> PageView;
    /// Take an invitation, only while the Box is waiting for one (BX-57).
    fn enter_invitation(&self, text: &str) -> Entered;
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// The name a visitor reached the page by, without its port, if it is made
/// only of characters a name or an address uses.
fn host_name(host: &str) -> Option<&str> {
    let name = if host.starts_with('[') {
        let end = host.find(']')?;
        host.get(..=end)?
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    let ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-.[]:".contains(&b));
    ok.then_some(name)
}

const STYLE: &str = "\
:root{color-scheme:light dark}\
body{font:16px/1.5 system-ui,sans-serif;margin:0;padding:1rem;\
background:Canvas;color:CanvasText}\
main{max-width:34rem;margin:0 auto}\
h1{font-size:1.4rem;margin:.2rem 0 1rem}\
h2{font-size:1.15rem;margin:0 0 .3rem}\
section{border-left:.35rem solid #888;padding:.5rem .9rem;margin:0 0 1rem;\
background:color-mix(in srgb,CanvasText 6%,Canvas)}\
.info{border-color:#3b82f6}.good{border-color:#16a34a}\
.stop{border-color:#dc2626}.ended{border-color:#888}\
p{margin:.3rem 0}\
dl{display:grid;grid-template-columns:max-content 1fr;gap:.2rem 1rem}\
dt{font-weight:600}dd{margin:0;overflow-wrap:anywhere}\
input[type=text]{width:100%;box-sizing:border-box;font:inherit;padding:.5rem}\
button{font:inherit;padding:.5rem 1rem;margin-top:.5rem}\
label{display:block;font-weight:600;margin-bottom:.3rem}";

fn tone_class(tone: Tone) -> &'static str {
    match tone {
        Tone::Info => "info",
        Tone::Good => "good",
        Tone::Stop => "stop",
        Tone::Ended => "ended",
    }
}

fn words_section(words: &Words) -> String {
    format!(
        "<section class=\"{}\"><h2>{}</h2><p>{}</p><p><strong>Next:</strong> {}</p></section>",
        tone_class(words.tone),
        escape(&words.heading),
        escape(&words.what),
        escape(&words.next),
    )
}

/// The home page for `view`, with the words of what came of a posted
/// invitation above the state when there is one (BX-55, BX-57). `host` is
/// the `Host` header of the request, used only to say where the devices
/// reach the relay.
pub fn render_home(view: &PageView, host: &str, outcome: Option<&Words>) -> String {
    let mut out = String::from(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <meta name=\"color-scheme\" content=\"light dark\">\
         <title>OpenBMT Box</title><style>",
    );
    out.push_str(STYLE);
    out.push_str("</style></head><body><main><h1>OpenBMT Box</h1>");
    if let Some(words) = outcome {
        out.push_str(&words_section(words));
    }
    out.push_str(&words_section(&view.status.page_words()));
    if view.store_full {
        out.push_str(&words_section(&store_full_words()));
    }
    if view.status == BoxStatus::Unpaired {
        out.push_str(&format!(
            "<form method=\"post\" action=\"/invitation\">\
             <label for=\"inv\">Invitation from the Key app</label>\
             <input id=\"inv\" type=\"text\" name=\"{FIELD}\" required maxlength=\"64\" \
             autocomplete=\"off\" autocapitalize=\"characters\" spellcheck=\"false\">\
             <button type=\"submit\">Give it to the Box</button></form>"
        ));
    }
    let relay = match host_name(host) {
        Some(name) => format!("{name}:{}", view.relay_port),
        None => format!("this computer's address, port {}", view.relay_port),
    };
    out.push_str("<dl>");
    out.push_str(&format!(
        "<dt>Version</dt><dd>{}</dd>",
        escape(view.version)
    ));
    out.push_str(&format!(
        "<dt>Relay address</dt><dd>{}</dd>",
        escape(&relay)
    ));
    if let Some(shop) = view.shop {
        out.push_str(&format!(
            "<dt>Shop</dt><dd>{}</dd>",
            escape(&shop.join(" "))
        ));
    }
    out.push_str(&format!(
        "<dt>Devices on the list</dt><dd>{}</dd>",
        view.devices
    ));
    out.push_str("</dl>");
    if let Some(note) = &view.last_note {
        out.push_str("<h2>Last thing the Box noted</h2>");
        out.push_str(&words_section(note));
    }
    out.push_str("</main></body></html>");
    out
}

/// The headers every answer carries (BX-56).
const SECURITY_HEADERS: &str = "\
Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; \
form-action 'self'; frame-ancestors 'none'\r\n\
X-Content-Type-Options: nosniff\r\n\
Referrer-Policy: no-referrer\r\n\
Cache-Control: no-store\r\n\
Connection: close\r\n";

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        401 => "Unauthorized",
        409 => "Conflict",
        413 => "Content Too Large",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

/// A whole answer: status line, headers and body.
pub fn response(status: u16, content_type: &str, extra: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status} {}\r\n{SECURITY_HEADERS}{extra}Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\r\n{body}",
        reason(status),
        body.len()
    )
    .into_bytes()
}

/// The answer to a refused request: the status and a fixed short text, with
/// no detail (BX-54).
pub fn refusal_response(refusal: Refusal) -> Vec<u8> {
    let (text, extra) = match refusal {
        Refusal::BadRequest => ("Bad request\n", ""),
        Refusal::NotFound => ("Not found\n", ""),
        Refusal::MethodNotAllowed(Route::Home) => ("Method not allowed\n", "Allow: GET\r\n"),
        Refusal::MethodNotAllowed(Route::Invitation) => ("Method not allowed\n", "Allow: POST\r\n"),
        Refusal::TooLarge => ("Request too large\n", ""),
    };
    response(refusal.status(), "text/plain; charset=utf-8", extra, text)
}

fn html(status: u16, body: &str) -> Vec<u8> {
    response(status, "text/html; charset=utf-8", "", body)
}

/// The answer to a request that passed the reader.
pub fn answer(backend: &dyn PageBackend, request: &Request) -> Vec<u8> {
    match request.route {
        Route::Home => html(200, &render_home(&backend.view(), &request.host, None)),
        Route::Invitation => {
            if !same_origin(
                &request.host,
                request.origin.as_deref(),
                request.referer.as_deref(),
            ) {
                let words = PageAnswer::NotFromThisPage.words();
                return html(
                    403,
                    &render_home(&backend.view(), &request.host, Some(&words)),
                );
            }
            let text = match form_field(&request.body, FIELD) {
                Ok(text) => text.unwrap_or_default(),
                Err(BadForm) => return refusal_response(Refusal::BadRequest),
            };
            let (status, words) = match backend.enter_invitation(&text) {
                Entered::Taken => (200, ConsoleAnswer::InvitationTaken.words()),
                Entered::Held => (409, PageAnswer::InvitationHeld.words()),
                Entered::Refused(e) => (invitation_page_status(&e), e.words()),
            };
            html(
                status,
                &render_home(&backend.view(), &request.host, Some(&words)),
            )
        }
    }
}

fn invitation_page_status(e: &BoxError) -> u16 {
    match e {
        BoxError::Invitation(_) | BoxError::InvitationExpired => 422,
        BoxError::AlreadyPaired | BoxError::TurnedOff => 409,
        _ => 500,
    }
}

/// Counts a connection while it lives.
pub(crate) struct Slot(pub(crate) Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A running status page. Dropping it stops it.
pub struct StatusPage {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl StatusPage {
    /// Serve `backend` on a listener the caller bound.
    pub fn start(
        listener: TcpListener,
        backend: Arc<dyn PageBackend>,
        cfg: PageConfig,
    ) -> io::Result<Self> {
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        let accept = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("page-accept".into())
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

impl Drop for StatusPage {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(
    listener: TcpListener,
    backend: Arc<dyn PageBackend>,
    cfg: PageConfig,
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
            .name("page-conn".into())
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
) -> Result<Request, Refusal> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(request) = parse_request(&buf)? {
            return Ok(request);
        }
        if stop.load(Ordering::SeqCst) {
            return Err(Refusal::BadRequest);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left.min(POLL))).is_err() {
            return Err(Refusal::BadRequest);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err(Refusal::BadRequest),
            Ok(n) => buf.extend_from_slice(chunk.get(..n).unwrap_or(&[])),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return Err(Refusal::BadRequest),
        }
    }
}

fn serve(mut stream: TcpStream, backend: &dyn PageBackend, cfg: PageConfig, stop: &AtomicBool) {
    if stream.set_nonblocking(false).is_err() || stream.set_write_timeout(Some(cfg.write)).is_err()
    {
        return;
    }
    let bytes = match read_request(&mut stream, Instant::now() + cfg.read, stop) {
        Ok(request) => answer(backend, &request),
        // A connection that went quiet or closed gets no answer.
        Err(Refusal::BadRequest) if stop.load(Ordering::SeqCst) => return,
        Err(refusal) => refusal_response(refusal),
    };
    if stream.write_all(&bytes).is_ok() {
        let _ = stream.flush();
    }
    let _ = stream.shutdown(Shutdown::Write);
}
