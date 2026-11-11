//! The Box's way to a POS vendor over HTTPS (BX-44 to BX-48,
//! [rules](../../../docs/BOX-RULES.md) section 4).
//!
//! The Box has no TLS or HTTP library. It runs the system's `curl` as a child
//! process, once for each request. [`HttpsRunner`] is the one function the
//! rest of the Box calls; [`CurlRunner`] is the real one and a test puts its
//! own in its place. What curl is told is built by [`invocation`] and can be
//! looked at without running anything: the arguments hold no header and no
//! body, and the config text that goes to curl's standard input holds them
//! all, the sign-in included.
//!
//! [`Transport`] is what the connector and the POS side share: whether the
//! shop's settings say Tor is on, and why the last request failed, in the
//! words of a [`HttpsRefusal`].

use std::fmt;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use openbmt_pos::PosError;

/// The curl the Box runs when no path is set (BX-44): the one on `PATH`.
pub const DEFAULT_CURL: &str = "curl";
/// The SOCKS5 proxy of a local Tor when none is set (BX-47).
pub const DEFAULT_SOCKS: &str = "127.0.0.1:9050";
/// Seconds a whole request may take (BX-46).
pub const TIMEOUT_SECS: u64 = 30;
/// Seconds the connection may take to be made (BX-46).
pub const CONNECT_TIMEOUT_SECS: u64 = 15;
/// Most bytes of a reply taken (BX-46): the bound of PS-90.
pub const MAX_REPLY_BYTES: usize = 8 * 1024 * 1024;
/// Most bytes of a request body (BX-46).
pub const MAX_BODY_BYTES: usize = 16 * 1024;
/// Most bytes of a URL, of a header or of a path to curl (BX-46).
pub const MAX_TEXT_BYTES: usize = 2_048;
/// Most headers on a request (BX-46).
pub const MAX_HEADERS: usize = 16;
/// Seconds the Box waits after a 429 when the POS did not say (BX-48).
pub const RATE_LIMIT_WAIT_SECS: u64 = 60;
/// Most bytes of curl's own error text kept.
const MAX_ERROR_TEXT: usize = 200;

/// The two methods the readers need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// A read with no body.
    Get,
    /// A search, with a body.
    Post,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// One request, whole (BX-44).
#[derive(Clone, PartialEq, Eq)]
pub struct HttpsRequest {
    /// The method.
    pub method: Method,
    /// The URL, which must start with `https://`.
    pub url: String,
    /// The headers, the sign-in among them.
    pub headers: Vec<(String, String)>,
    /// The body, for a POST.
    pub body: Option<String>,
    /// A SOCKS5 proxy, `host:port`, that the request goes through and that
    /// resolves the name (BX-47). With one set there is no other way out.
    pub socks5: Option<String>,
    /// Seconds the whole request may take.
    pub timeout_secs: u64,
    /// Most bytes of reply taken.
    pub max_reply: usize,
}

impl HttpsRequest {
    /// A request with the Box's limits (BX-46) and no proxy.
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
            socks5: None,
            timeout_secs: TIMEOUT_SECS,
            max_reply: MAX_REPLY_BYTES,
        }
    }
}

// A request holds the sign-in in its headers: it never prints them.
impl fmt::Debug for HttpsRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpsRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &self.headers.len())
            .field("body_bytes", &self.body.as_ref().map(String::len))
            .field("socks5", &self.socks5)
            .finish()
    }
}

/// What came back: the HTTP status and the body, at most the cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpsReply {
    /// The HTTP status.
    pub status: u16,
    /// The body.
    pub body: Vec<u8>,
}

impl HttpsReply {
    /// BX-48: the body of an answer with status 200 to 299, or the refusal
    /// the status makes.
    pub fn into_body(self) -> Result<Vec<u8>, HttpsRefusal> {
        match self.status {
            200..=299 => Ok(self.body),
            401 | 403 => Err(HttpsRefusal::TokenRefused {
                status: self.status,
            }),
            429 => Err(HttpsRefusal::RateLimited {
                retry_after_secs: RATE_LIMIT_WAIT_SECS,
            }),
            300..=399 => Err(HttpsRefusal::Redirected {
                status: self.status,
            }),
            status => Err(HttpsRefusal::Status { status }),
        }
    }
}

/// The kind of a refusal, one word each. The tag is what a stored note
/// carries, so that the words can be made again when the note is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpsKind {
    /// There is no curl at the path (BX-44).
    CurlMissing,
    /// curl could not be started (BX-44).
    CurlNotRun,
    /// The request itself was refused before curl was run (BX-45, BX-46).
    BadRequest,
    /// The request took too long (BX-46).
    Timeout,
    /// The POS could not be reached (BX-46).
    Unreachable,
    /// The TLS connection was refused (BX-46).
    Tls,
    /// The reply was larger than the cap (BX-46).
    ReplyTooLarge,
    /// curl failed in some other way (BX-46).
    CurlFailed,
    /// curl's output was not a reply and a status (BX-46).
    BadOutput,
    /// The Tor proxy could not be used (BX-47).
    ProxyUnreachable,
    /// The POS answered 401 or 403 (BX-48).
    TokenRefused,
    /// The POS answered 429 (BX-48).
    RateLimited,
    /// The POS answered with a redirect (BX-48).
    Redirected,
    /// The POS answered with another status (BX-48).
    Status,
    /// The reply was refused by the reader (BX-49).
    Unreadable,
}

impl HttpsKind {
    /// Every kind, for the list of words.
    pub const ALL: [HttpsKind; 15] = [
        Self::CurlMissing,
        Self::CurlNotRun,
        Self::BadRequest,
        Self::Timeout,
        Self::Unreachable,
        Self::Tls,
        Self::ReplyTooLarge,
        Self::CurlFailed,
        Self::BadOutput,
        Self::ProxyUnreachable,
        Self::TokenRefused,
        Self::RateLimited,
        Self::Redirected,
        Self::Status,
        Self::Unreadable,
    ];

    /// The tag a note carries.
    pub fn tag(self) -> &'static str {
        match self {
            Self::CurlMissing => "curl-missing",
            Self::CurlNotRun => "curl-not-run",
            Self::BadRequest => "bad-request",
            Self::Timeout => "timeout",
            Self::Unreachable => "unreachable",
            Self::Tls => "tls",
            Self::ReplyTooLarge => "reply-too-large",
            Self::CurlFailed => "curl-failed",
            Self::BadOutput => "bad-output",
            Self::ProxyUnreachable => "proxy-unreachable",
            Self::TokenRefused => "token-refused",
            Self::RateLimited => "rate-limited",
            Self::Redirected => "redirected",
            Self::Status => "status",
            Self::Unreadable => "unreadable",
        }
    }

    /// The kind a tag names.
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.tag() == tag)
    }

    /// The rule the kind is about.
    pub fn code(self) -> &'static str {
        match self {
            Self::CurlMissing | Self::CurlNotRun => "BX-44",
            Self::BadRequest => "BX-45",
            Self::Timeout
            | Self::Unreachable
            | Self::Tls
            | Self::ReplyTooLarge
            | Self::CurlFailed
            | Self::BadOutput => "BX-46",
            Self::ProxyUnreachable => "BX-47",
            Self::TokenRefused | Self::RateLimited | Self::Redirected | Self::Status => "BX-48",
            Self::Unreadable => "BX-49",
        }
    }
}

/// Why a request did not give a reply the POS side can read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpsRefusal {
    /// There is no curl at the path.
    CurlMissing {
        /// The path or name that was tried.
        path: String,
    },
    /// curl could not be started.
    CurlNotRun {
        /// The path.
        path: String,
        /// What the system said.
        kind: io::ErrorKind,
    },
    /// The request was refused before curl was run, with the reason.
    BadRequest(String),
    /// curl's time limit ran out.
    Timeout,
    /// The name or the server could not be reached, or the connection broke.
    Unreachable {
        /// curl's exit code.
        code: i32,
    },
    /// The TLS connection could not be made or the certificate was refused.
    Tls {
        /// curl's exit code.
        code: i32,
    },
    /// The reply is larger than the cap.
    ReplyTooLarge {
        /// The cap in bytes.
        cap: usize,
    },
    /// curl failed in another way.
    CurlFailed {
        /// curl's exit code, when it exited and was not ended by a signal.
        code: Option<i32>,
        /// The first line of curl's own message, cut short.
        text: String,
    },
    /// curl said it was done but its output has no status at the end.
    BadOutput,
    /// The SOCKS5 proxy could not be used and nothing else was tried.
    ProxyUnreachable {
        /// The proxy, `host:port`.
        proxy: String,
    },
    /// The POS refused the sign-in.
    TokenRefused {
        /// The HTTP status, 401 or 403.
        status: u16,
    },
    /// The POS asked to be called less often.
    RateLimited {
        /// Seconds to wait.
        retry_after_secs: u64,
    },
    /// The POS answered with a redirect, which is not followed.
    Redirected {
        /// The HTTP status.
        status: u16,
    },
    /// Any other status than 200 to 299.
    Status {
        /// The HTTP status.
        status: u16,
    },
    /// The reply came but the reader refused it.
    Unreadable(String),
}

impl HttpsRefusal {
    /// The kind.
    pub fn kind(&self) -> HttpsKind {
        match self {
            Self::CurlMissing { .. } => HttpsKind::CurlMissing,
            Self::CurlNotRun { .. } => HttpsKind::CurlNotRun,
            Self::BadRequest(_) => HttpsKind::BadRequest,
            Self::Timeout => HttpsKind::Timeout,
            Self::Unreachable { .. } => HttpsKind::Unreachable,
            Self::Tls { .. } => HttpsKind::Tls,
            Self::ReplyTooLarge { .. } => HttpsKind::ReplyTooLarge,
            Self::CurlFailed { .. } => HttpsKind::CurlFailed,
            Self::BadOutput => HttpsKind::BadOutput,
            Self::ProxyUnreachable { .. } => HttpsKind::ProxyUnreachable,
            Self::TokenRefused { .. } => HttpsKind::TokenRefused,
            Self::RateLimited { .. } => HttpsKind::RateLimited,
            Self::Redirected { .. } => HttpsKind::Redirected,
            Self::Status { .. } => HttpsKind::Status,
            Self::Unreadable(_) => HttpsKind::Unreadable,
        }
    }

    /// What the connector gives the sync: the nearest [`PosError`].
    pub fn pos_error(&self) -> PosError {
        match self {
            Self::TokenRefused { .. } => PosError::SignInRefused,
            Self::RateLimited { retry_after_secs } => PosError::RateLimited {
                retry_after_secs: *retry_after_secs,
            },
            Self::CurlMissing { .. }
            | Self::CurlNotRun { .. }
            | Self::Timeout
            | Self::Unreachable { .. }
            | Self::Tls { .. }
            | Self::ProxyUnreachable { .. }
            | Self::CurlFailed { .. } => PosError::Unavailable,
            Self::Status { status } if *status >= 500 => PosError::Unavailable,
            Self::BadRequest(_) => PosError::BadResponse("the request was refused"),
            Self::ReplyTooLarge { .. } => PosError::BadResponse("the reply is too large"),
            Self::BadOutput => PosError::BadResponse("curl gave no status"),
            Self::Redirected { .. } => PosError::BadResponse("the POS redirected the request"),
            Self::Status { .. } => PosError::BadResponse("the POS answered with an error status"),
            Self::Unreadable(_) => PosError::BadResponse("the reply was refused by the reader"),
        }
    }

    /// The text after the kind's code, which is what the note carries as its
    /// detail.
    pub fn detail(&self) -> String {
        match self {
            Self::CurlMissing { path } => {
                format!("curl is not installed on this Box (looked for \"{path}\")")
            }
            Self::CurlNotRun { path, kind } => {
                format!("curl at \"{path}\" could not be started: {kind}")
            }
            Self::BadRequest(why) => format!("the request was not made: {why}"),
            Self::Timeout => format!("the request took longer than {TIMEOUT_SECS} seconds"),
            Self::Unreachable { code } => {
                format!("the POS could not be reached (curl exit {code})")
            }
            Self::Tls { code } => format!("the encrypted connection failed (curl exit {code})"),
            Self::ReplyTooLarge { cap } => format!("the reply is larger than {cap} bytes"),
            Self::CurlFailed { code, text } => match code {
                Some(code) => format!("curl failed (exit {code}): {text}"),
                None => format!("curl was ended by a signal: {text}"),
            },
            Self::BadOutput => "curl's output has no status at its end".to_string(),
            Self::ProxyUnreachable { proxy } => {
                format!("the Tor proxy at {proxy} could not be used and nothing else was tried")
            }
            Self::TokenRefused { status } => format!("the POS answered {status}"),
            Self::RateLimited { retry_after_secs } => {
                format!("the POS answered 429: wait {retry_after_secs} seconds")
            }
            Self::Redirected { status } => {
                format!("the POS answered {status} and redirects are not followed")
            }
            Self::Status { status } => format!("the POS answered {status}"),
            Self::Unreadable(why) => format!("the reply was refused: {why}"),
        }
    }

    /// The line a note carries: the tag, the code and the detail. The words
    /// are made from the tag when the note is shown.
    pub fn note(&self) -> String {
        let kind = self.kind();
        format!(
            "the POS was not reached ({}): {}: {}",
            kind.tag(),
            kind.code(),
            self.detail()
        )
    }
}

impl fmt::Display for HttpsRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind().code(), self.detail())
    }
}

impl std::error::Error for HttpsRefusal {}

/// What the connector and the POS side share (BX-47): whether Tor is on in
/// the settings in force, and why the last request failed.
#[derive(Debug)]
pub struct Transport {
    tor: AtomicBool,
    last: Mutex<Option<HttpsRefusal>>,
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport {
    /// Tor is taken as on until the settings say otherwise: a Box that does
    /// not know goes the safe way (BX-47).
    pub fn new() -> Self {
        Self {
            tor: AtomicBool::new(true),
            last: Mutex::new(None),
        }
    }

    fn last(&self) -> MutexGuard<'_, Option<HttpsRefusal>> {
        self.last.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether requests go through the Tor proxy.
    pub fn tor(&self) -> bool {
        self.tor.load(Ordering::SeqCst)
    }

    /// Set from the settings in force.
    pub fn set_tor(&self, on: bool) {
        self.tor.store(on, Ordering::SeqCst);
    }

    /// Keep why a request failed.
    pub fn record(&self, why: HttpsRefusal) {
        *self.last() = Some(why);
    }

    /// Forget the last failure.
    pub fn clear(&self) {
        *self.last() = None;
    }

    /// Take the last failure.
    pub fn take(&self) -> Option<HttpsRefusal> {
        self.last().take()
    }
}

/// The one thing the rest of the Box asks of the network (BX-44).
pub trait HttpsRunner: Send {
    /// Make the request and give the status and the body. A status other
    /// than 200 to 299 is an answer here; [`HttpsReply::into_body`] makes it
    /// a refusal.
    fn run(&mut self, request: &HttpsRequest) -> Result<HttpsReply, HttpsRefusal>;
}

/// What curl is run with: the arguments, and the text it reads from its
/// standard input as a config (BX-45).
#[derive(Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The arguments after the program's name. No header and no body.
    pub args: Vec<String>,
    /// The config: each header and the body.
    pub config: String,
}

impl fmt::Debug for Invocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Invocation")
            .field("args", &self.args)
            .field("config_bytes", &self.config.len())
            .finish()
    }
}

fn bad(why: &str) -> HttpsRefusal {
    HttpsRefusal::BadRequest(why.to_string())
}

/// A string as curl's config file reads it between double quotes.
fn quoted(text: &str, what: &str) -> Result<String, HttpsRefusal> {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                return Err(bad(&format!("{what} holds a control character")));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    Ok(out)
}

fn plain_url(url: &str) -> bool {
    url.starts_with("https://")
        && url.len() <= MAX_TEXT_BYTES
        && url.bytes().all(|b| b.is_ascii_graphic())
}

fn plain_proxy(proxy: &str) -> bool {
    let Some((host, port)) = proxy.rsplit_once(':') else {
        return false;
    };
    !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && port.parse::<u16>().is_ok_and(|p| p > 0)
}

fn token_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// BX-44 to BX-47: what curl is run with for `request`. The arguments are
/// the flags of BX-46 and the URL, `--socks5-hostname` when a proxy is set;
/// the headers and the body are in the config text only.
pub fn invocation(request: &HttpsRequest) -> Result<Invocation, HttpsRefusal> {
    if !plain_url(&request.url) {
        return Err(bad("the URL is not a plain https URL"));
    }
    if request.headers.len() > MAX_HEADERS {
        return Err(bad("too many headers"));
    }
    if request.timeout_secs == 0 || request.max_reply == 0 {
        return Err(bad("a limit is zero"));
    }
    let mut config = String::new();
    for (name, value) in &request.headers {
        if !token_name(name) {
            return Err(bad("a header name is not valid"));
        }
        if value.len() > MAX_TEXT_BYTES {
            return Err(bad("a header value is too long"));
        }
        // A line end in a value would start another header.
        if value.chars().any(char::is_control) {
            return Err(bad("a header value holds a control character"));
        }
        let line = quoted(&format!("{name}: {value}"), "a header")?;
        config.push_str("header = ");
        config.push_str(&line);
        config.push('\n');
    }
    match (&request.body, request.method) {
        (Some(body), Method::Post) => {
            if body.len() > MAX_BODY_BYTES {
                return Err(bad("the body is too long"));
            }
            // curl reads "@name" as a file to send.
            if body.starts_with('@') {
                return Err(bad("the body starts with @"));
            }
            config.push_str("data-binary = ");
            config.push_str(&quoted(body, "the body")?);
            config.push('\n');
        }
        (None, Method::Get) => {}
        _ => return Err(bad("a GET has no body and a POST has one")),
    }
    // -q first, so that no config file of the user's changes the rest.
    let mut args: Vec<String> = [
        "-q",
        "--config",
        "-",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--tlsv1.2",
        "--max-time",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(request.timeout_secs.to_string());
    args.push("--connect-timeout".into());
    args.push(CONNECT_TIMEOUT_SECS.min(request.timeout_secs).to_string());
    args.push("--max-filesize".into());
    args.push(request.max_reply.to_string());
    args.push("--request".into());
    args.push(request.method.as_str().into());
    args.push("--write-out".into());
    args.push("%{http_code}".into());
    if let Some(proxy) = &request.socks5 {
        if !plain_proxy(proxy) {
            return Err(bad("the Tor proxy is not host:port"));
        }
        args.push("--socks5-hostname".into());
        args.push(proxy.clone());
    }
    args.push(request.url.clone());
    Ok(Invocation { args, config })
}

/// The status at the end of curl's output, and the body before it.
pub(crate) fn split_status(out: &[u8]) -> Result<(u16, &[u8]), HttpsRefusal> {
    let at = out.len().checked_sub(3).ok_or(HttpsRefusal::BadOutput)?;
    let (body, code) = out.split_at(at);
    if !code.iter().all(u8::is_ascii_digit) {
        return Err(HttpsRefusal::BadOutput);
    }
    let status = code.iter().fold(0u16, |n, d| n * 10 + u16::from(d - b'0'));
    if !(100..=599).contains(&status) {
        return Err(HttpsRefusal::BadOutput);
    }
    Ok((status, body))
}

/// curl's exit codes that say the TLS connection failed.
const TLS_EXITS: [i32; 11] = [35, 51, 53, 54, 58, 59, 60, 77, 80, 82, 83];

/// BX-46, BX-47: what curl's exit code means.
pub(crate) fn exit_refusal(code: Option<i32>, proxy: Option<&str>, text: String) -> HttpsRefusal {
    if let (Some(proxy), Some(5 | 7 | 97)) = (proxy, code) {
        return HttpsRefusal::ProxyUnreachable {
            proxy: proxy.to_string(),
        };
    }
    match code {
        Some(28) => HttpsRefusal::Timeout,
        Some(63) => HttpsRefusal::ReplyTooLarge {
            cap: MAX_REPLY_BYTES,
        },
        Some(c) if TLS_EXITS.contains(&c) => HttpsRefusal::Tls { code: c },
        Some(c @ (6 | 7 | 52 | 55 | 56)) => HttpsRefusal::Unreachable { code: c },
        code => HttpsRefusal::CurlFailed { code, text },
    }
}

/// The first line of curl's error text, with no control character, cut
/// short.
pub(crate) fn error_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let line = text.lines().next().unwrap_or("");
    line.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_ERROR_TEXT)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Read at most `cap` bytes; `None` if there is more.
pub(crate) fn read_capped(from: &mut impl Read, cap: usize) -> io::Result<Option<Vec<u8>>> {
    let mut out = Vec::new();
    let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
    from.take(limit).read_to_end(&mut out)?;
    Ok((out.len() <= cap).then_some(out))
}

/// The real runner: the system's curl, as a child process (BX-44).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlRunner {
    curl: String,
}

impl CurlRunner {
    /// Run the curl at `path`; [`DEFAULT_CURL`] finds it on `PATH`.
    pub fn new(path: impl Into<String>) -> Self {
        Self { curl: path.into() }
    }
}

impl HttpsRunner for CurlRunner {
    fn run(&mut self, request: &HttpsRequest) -> Result<HttpsReply, HttpsRefusal> {
        let invocation = invocation(request)?;
        let mut command = Command::new(&self.curl);
        // The child sees nothing of the Box's environment: no proxy
        // variable can take a request round the Tor proxy, and nothing but
        // the certificate locations is passed on (BX-45).
        command.args(&invocation.args).env_clear();
        for name in ["SSL_CERT_FILE", "SSL_CERT_DIR"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                io::ErrorKind::NotFound => HttpsRefusal::CurlMissing {
                    path: self.curl.clone(),
                },
                kind => HttpsRefusal::CurlNotRun {
                    path: self.curl.clone(),
                    kind,
                },
            })?;
        // A curl that ended before it read its config is judged by its exit
        // code below.
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(invocation.config.as_bytes());
        }
        let cap = request.max_reply;
        let out = match child.stdout.take() {
            Some(mut stdout) => read_capped(&mut stdout, cap.saturating_add(3)),
            None => Ok(Some(Vec::new())),
        };
        let out = match out {
            Ok(Some(out)) => out,
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(HttpsRefusal::ReplyTooLarge { cap });
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(HttpsRefusal::CurlNotRun {
                    path: self.curl.clone(),
                    kind: e.kind(),
                });
            }
        };
        let mut err = Vec::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.by_ref().take(4_096).read_to_end(&mut err);
        }
        let status = child.wait().map_err(|e| HttpsRefusal::CurlNotRun {
            path: self.curl.clone(),
            kind: e.kind(),
        })?;
        if !status.success() {
            return Err(exit_refusal(
                status.code(),
                request.socks5.as_deref(),
                error_text(&err),
            ));
        }
        let (status, body) = split_status(&out)?;
        Ok(HttpsReply {
            status,
            body: body.to_vec(),
        })
    }
}
