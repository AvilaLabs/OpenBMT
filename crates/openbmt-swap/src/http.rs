//! The request to the provider and its reply, as bytes (SW-80 to SW-82).
//!
//! A minimal HTTP/1.1 codec with no socket and no TLS: the provider is
//! reached over its onion address only, through a proxy the caller owns.
//! The library has no provider built in. The setting names one, and the
//! default setting is off.

use crate::error::Refused;
use crate::onion::parse_onion_name;
use crate::request::Request;
use crate::MAX_HTTP_BYTES;

/// The most bytes of the provider's label.
const MAX_NAME_BYTES: usize = 32;
/// The most bytes of a path, prefix included.
const MAX_PATH_BYTES: usize = 200;
/// The most header lines of a reply.
const MAX_HEADERS: usize = 64;

/// A swap provider, as the shop's setting holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    name: String,
    onion: String,
    path_prefix: String,
}

/// SW-82: the shop's swap setting. The default is off, and the library has
/// no provider of its own to turn on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Setting(Option<Provider>);

impl Setting {
    /// The swap service is on, with this provider.
    pub fn on(provider: Provider) -> Self {
        Self(Some(provider))
    }

    /// Whether a provider is set.
    pub fn is_on(&self) -> bool {
        self.0.is_some()
    }

    /// The provider, or the refusal that the service is off.
    pub fn provider(&self) -> Result<&Provider, Refused> {
        self.0.as_ref().ok_or(Refused::ProviderOff)
    }
}

fn path_chars_ok(path: &str) -> bool {
    path.len() <= MAX_PATH_BYTES
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'.' | b'-'))
        && !path.contains("//")
        && !path.contains("..")
}

impl Provider {
    /// SW-82: a provider from its label, its pinned v3 onion host name
    /// (`<56 characters>.onion`, checksum and version checked) and the path
    /// prefix its API is served under: empty, or starting with `/` and not
    /// ending with one (`/api`).
    pub fn new(name: &str, onion_host: &str, path_prefix: &str) -> Result<Self, Refused> {
        if name.is_empty()
            || name.len() > MAX_NAME_BYTES
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-' | b'_' | b'.'))
        {
            return Err(Refused::BadProvider("name"));
        }
        if parse_onion_name(onion_host).is_none() {
            return Err(Refused::BadProvider("onion name"));
        }
        if !path_prefix.is_empty()
            && (!path_prefix.starts_with('/')
                || path_prefix.ends_with('/')
                || !path_chars_ok(path_prefix))
        {
            return Err(Refused::BadProvider("path prefix"));
        }
        Ok(Self {
            name: name.to_string(),
            onion: onion_host.to_string(),
            path_prefix: path_prefix.to_string(),
        })
    }

    /// The label.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The pinned onion host name.
    pub fn onion(&self) -> &str {
        &self.onion
    }

    /// The path prefix.
    pub fn path_prefix(&self) -> &str {
        &self.path_prefix
    }

    fn target(&self, path: &str) -> Result<String, Refused> {
        if !path.starts_with('/') || path.ends_with('/') || !path_chars_ok(path) {
            return Err(Refused::BadHttpRequest("path"));
        }
        let full = format!("{}{}", self.path_prefix, path);
        if full.len() > MAX_PATH_BYTES {
            return Err(Refused::BadHttpRequest("path"));
        }
        Ok(full)
    }

    /// SW-80: a `GET` of `path` (for example `/v2/swap/reverse`). The only
    /// headers are `Host` and `Connection: close`.
    pub fn get(&self, path: &str) -> Result<Vec<u8>, Refused> {
        let target = self.target(path)?;
        Ok(format!(
            "GET {target} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            self.onion
        )
        .into_bytes())
    }

    /// SW-80: a `POST` of the JSON `body` to `path`. The only headers are
    /// `Host`, `Content-Type`, `Content-Length` and `Connection: close`.
    pub fn post(&self, path: &str, body: &[u8]) -> Result<Vec<u8>, Refused> {
        let target = self.target(path)?;
        if body.len() > MAX_HTTP_BYTES {
            return Err(Refused::BadHttpRequest("body"));
        }
        let mut out = format!(
            "POST {target} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            self.onion,
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        Ok(out)
    }

    /// SW-80: the request for the service's live terms.
    pub fn pairs_request(&self) -> Result<Vec<u8>, Refused> {
        self.get("/v2/swap/reverse")
    }

    /// SW-80: the request to make a reverse swap.
    pub fn create_request(&self, request: &Request) -> Result<Vec<u8>, Refused> {
        self.post("/v2/swap/reverse", request.to_json().as_bytes())
    }

    /// SW-80: the request for a swap's status. The id must be a valid one
    /// (SW-12).
    pub fn status_request(&self, id: &str) -> Result<Vec<u8>, Refused> {
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(Refused::BadHttpRequest("id"));
        }
        self.get(&format!("/v2/swap/{id}"))
    }
}

/// A reply, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    status: u16,
    body: Vec<u8>,
}

impl Response {
    /// The status code.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The body, whatever the status.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// SW-81: the body of a reply with status 200 or 201 (the service
    /// answers a made swap with 201); any other status is refused.
    pub fn ok_body(&self) -> Result<&[u8], Refused> {
        if matches!(self.status, 200 | 201) {
            Ok(&self.body)
        } else {
            Err(Refused::HttpStatus(self.status))
        }
    }
}

fn bad(why: &'static str) -> Refused {
    Refused::BadHttpReply(why)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn is_token(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// SW-81: read a whole reply (what the connection gave until the provider
/// closed it). The status line, the headers and a body by `Content-Length`
/// or `Transfer-Encoding: chunked`, at most `MAX_HTTP_BYTES`. A reply the
/// reader does not understand is refused, and so are bytes after the body.
pub fn parse_response(bytes: &[u8]) -> Result<Response, Refused> {
    if bytes.len() > MAX_HTTP_BYTES {
        return Err(bad("longer than the limit"));
    }
    let end = find(bytes, b"\r\n\r\n").ok_or(bad("no end of headers"))?;
    let (head, rest) = bytes.split_at(end);
    let rest = rest.get(4..).ok_or(bad("no end of headers"))?;
    let head = std::str::from_utf8(head)
        .ok()
        .filter(|h| h.is_ascii())
        .ok_or(bad("headers are not ASCII"))?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or(bad("no status line"))?;
    let after = status_line
        .strip_prefix("HTTP/1.1 ")
        .ok_or(bad("not HTTP/1.1"))?;
    let (code, reason) = match after.split_once(' ') {
        Some((c, r)) => (c, r),
        None => (after, ""),
    };
    if code.len() != 3
        || !code.bytes().all(|b| b.is_ascii_digit())
        || reason.len() > 64
        || reason.bytes().any(|b| b.is_ascii_control())
    {
        return Err(bad("status line"));
    }
    let status: u16 = code.parse().map_err(|_| bad("status line"))?;
    if !(200..=599).contains(&status) {
        return Err(bad("status code"));
    }

    let (mut length, mut chunked) = (None::<usize>, false);
    let mut seen_te = false;
    let mut count = 0usize;
    for line in lines {
        count += 1;
        if count > MAX_HEADERS {
            return Err(bad("too many headers"));
        }
        if line.contains(['\r', '\n']) || line.starts_with([' ', '\t']) {
            return Err(bad("header line"));
        }
        let (name, value) = line.split_once(':').ok_or(bad("header line"))?;
        if !is_token(name) {
            return Err(bad("header name"));
        }
        let value = value.trim_matches([' ', '\t']);
        if value.bytes().any(|b| b.is_ascii_control() && b != b'\t') {
            return Err(bad("header value"));
        }
        match name.to_ascii_lowercase().as_str() {
            "content-length" => {
                if length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(bad("content length"));
                }
                length = Some(value.parse().map_err(|_| bad("content length"))?);
            }
            "transfer-encoding" => {
                if seen_te || !value.eq_ignore_ascii_case("chunked") {
                    return Err(bad("transfer encoding"));
                }
                seen_te = true;
                chunked = true;
            }
            "content-encoding" if !value.eq_ignore_ascii_case("identity") => {
                return Err(bad("content encoding"));
            }
            _ => {}
        }
    }
    let body = match (chunked, length) {
        (true, Some(_)) => return Err(bad("both length and chunks")),
        (true, None) => dechunk(rest)?,
        (false, Some(n)) => {
            if rest.len() < n {
                return Err(bad("body is cut short"));
            }
            if rest.len() > n {
                return Err(bad("bytes after the body"));
            }
            rest.to_vec()
        }
        (false, None) => return Err(bad("no length")),
    };
    Ok(Response { status, body })
}

/// The body of a chunked reply: hexadecimal sizes with no extension, and no
/// trailer after the last chunk.
fn dechunk(mut rest: &[u8]) -> Result<Vec<u8>, Refused> {
    let mut body = Vec::new();
    loop {
        let eol = find(rest, b"\r\n").ok_or(bad("chunk size"))?;
        let (size, after) = rest.split_at(eol);
        rest = after.get(2..).ok_or(bad("chunk size"))?;
        if size.is_empty() || size.len() > 8 || !size.iter().all(u8::is_ascii_hexdigit) {
            return Err(bad("chunk size"));
        }
        let size = usize::from_str_radix(std::str::from_utf8(size).map_err(|_| bad("chunk"))?, 16)
            .map_err(|_| bad("chunk size"))?;
        if size == 0 {
            return if rest == b"\r\n" {
                Ok(body)
            } else {
                Err(bad("bytes after the last chunk"))
            };
        }
        let (data, after) = rest
            .split_at_checked(size)
            .ok_or(bad("chunk is cut short"))?;
        rest = after.strip_prefix(b"\r\n").ok_or(bad("chunk end"))?;
        if body.len() + data.len() > MAX_HTTP_BYTES {
            return Err(bad("longer than the limit"));
        }
        body.extend_from_slice(data);
    }
}
