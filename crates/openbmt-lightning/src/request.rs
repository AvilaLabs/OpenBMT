//! The requests the crate asks the app to send and the replies the app hands
//! back. There is no network here: the app sends each request over HTTPS to
//! `Endpoint::host_port`, with the pinned certificate (LN-12), and hands in
//! the status and the bytes of the answer.

use std::fmt;

use openbmt_core::say::clean_foreign;
use openbmt_json::{parse, Limits, Value};

use crate::error::ReplyRefused;
use crate::text::unhex_array;

/// The largest reply the crate reads: a page of the node's invoices for one
/// hash or one label is a few kilobytes.
pub const MAX_REPLY_BYTES: usize = 256 * 1024;

/// The method of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// HTTP GET, no body.
    Get,
    /// HTTP POST with a JSON body (`Content-Type: application/json`).
    Post,
}

/// One request for the app to send: HTTPS, to the node's `host:port`, with
/// the one header that carries the credential. The path holds the query.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    /// GET or POST.
    pub method: Method,
    /// The path and query, beginning with `/`.
    pub path: String,
    /// The name of the header that carries the credential.
    pub header_name: &'static str,
    /// The value of that header. It is a secret: it is not in the `Debug`
    /// text and the app must not log it.
    pub header_value: String,
    /// The JSON text of the body of a POST.
    pub body: Option<String>,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("header_name", &self.header_name)
            .field("header_value", &"<not shown>")
            .field("body", &self.body)
            .finish()
    }
}

/// What the node answered: the HTTP status and the bytes of the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The HTTP status.
    pub status: u16,
    /// The body.
    pub body: Vec<u8>,
}

impl Reply {
    /// Whether the status is 200 to 299.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// The limits of the JSON reader for a reply of the node.
pub(crate) const LIMITS: Limits = Limits {
    max_bytes: MAX_REPLY_BYTES,
    max_depth: 16,
    max_values: 10_000,
};

/// The JSON of a body, if it has any.
pub(crate) fn json(reply: &Reply) -> Result<Value, ReplyRefused> {
    Ok(parse(&reply.body, &LIMITS)?)
}

/// The error of a reply that is not a success: the status and the node's own
/// text, cleaned. The text is the `message` of the JSON error both nodes
/// send, or empty when the body is something else.
pub(crate) fn node_said(reply: &Reply) -> ReplyRefused {
    let message = json(reply)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(clean_foreign))
        .unwrap_or_default();
    ReplyRefused::NodeSaid {
        status: reply.status,
        message,
    }
}

/// The JSON of a successful reply; any other status is the node's error.
pub(crate) fn success_json(reply: &Reply) -> Result<Value, ReplyRefused> {
    if reply.is_success() {
        json(reply)
    } else {
        Err(node_said(reply))
    }
}

/// A text field.
pub(crate) fn text<'a>(v: &'a Value, key: &'static str) -> Result<&'a str, ReplyRefused> {
    v.get(key)
        .ok_or(ReplyRefused::Missing(key))?
        .as_str()
        .ok_or(ReplyRefused::WrongType(key))
}

/// A whole number written as a plain JSON number, as Core Lightning writes
/// every amount and time (`json_add_u64` in `common/json_stream.c`). A text
/// is refused.
pub(crate) fn number(v: &Value, key: &'static str) -> Result<u64, ReplyRefused> {
    let field = v.get(key).ok_or(ReplyRefused::Missing(key))?;
    match field.as_u64() {
        Some(n) => Ok(n),
        None if field.as_i64().is_some() => Err(ReplyRefused::OutOfRange(key)),
        None => Err(ReplyRefused::WrongType(key)),
    }
}

/// A whole number written as a JSON text of digits, as LND's REST writes an
/// `int64` (the protojson rule). A JSON number is refused.
pub(crate) fn int64_text(v: &Value, key: &'static str) -> Result<u64, ReplyRefused> {
    let s = text(v, key)?;
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ReplyRefused::OutOfRange(key));
    }
    s.parse().map_err(|_| ReplyRefused::OutOfRange(key))
}

/// The `code` of the JSON error a node sends, if the body has one.
pub(crate) fn node_code(reply: &Reply) -> Option<i64> {
    json(reply)
        .ok()
        .and_then(|v| v.get("code").and_then(Value::as_i64))
}

/// A 32-byte value written in hex.
pub(crate) fn hash_hex(v: &Value, key: &'static str) -> Result<[u8; 32], ReplyRefused> {
    unhex_array(text(v, key)?).ok_or(ReplyRefused::OutOfRange(key))
}

/// The one header and the path of a call without a body.
pub(crate) fn get(path: String, header_name: &'static str, header_value: String) -> Request {
    Request {
        method: Method::Get,
        path,
        header_name,
        header_value,
        body: None,
    }
}

/// A POST with a JSON body.
pub(crate) fn post(
    path: &str,
    header_name: &'static str,
    header_value: String,
    body: String,
) -> Request {
    Request {
        method: Method::Post,
        path: path.to_string(),
        header_name,
        header_value,
        body: Some(body),
    }
}

/// The text as a JSON string, quoted and escaped.
pub(crate) fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
