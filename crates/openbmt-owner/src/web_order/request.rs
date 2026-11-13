//! WO-20: the HMAC-SHA256 of a request under the pairing secret, with a
//! time and a number used once.
//!
//! The message that is authenticated is, in order, each followed by a line
//! feed but the last: the text `OBM-WEB-REQUEST-1`, the method, the route,
//! the time in Unix seconds as decimal digits, the number used once in
//! lower-case hex and the SHA-256 of the body in lower-case hex.

use bitcoin::hashes::{hmac, sha256, Hash, HashEngine};

use super::pairing::SECRET_BYTES;
use super::{hex, WebOrderRefusal};
use crate::starter::{KernelRandom, KERNEL_BYTES};

/// The header that carries the time.
pub const HEADER_TIME: &str = "X-OBM-Time";
/// The header that carries the number used once.
pub const HEADER_NONCE: &str = "X-OBM-Nonce";
/// The header that carries the HMAC.
pub const HEADER_MAC: &str = "X-OBM-Mac";
/// The route of the owner phone's request for orders (WO-20).
pub const ROUTE_ORDERS: &str = "/openbmt/v1/orders";
/// The route of a signed range (WO-10).
pub const ROUTE_RANGE: &str = "/openbmt/v1/range";
/// The route of a signed notice (WO-30).
pub const ROUTE_NOTICE: &str = "/openbmt/v1/notice";
/// The bytes of the number used once.
pub const NONCE_BYTES: usize = 16;
/// The seconds either side of the store's clock within which a request's time
/// is accepted: 5 minutes.
pub const TIME_WINDOW_SECS: u64 = 300;

const TAG: &str = "OBM-WEB-REQUEST-1";

/// The three headers of an authenticated request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedRequest {
    /// The time, in Unix seconds.
    pub time: u64,
    /// The number used once.
    pub nonce: [u8; NONCE_BYTES],
    /// The HMAC.
    pub mac: [u8; 32],
}

/// The body of the request for orders: the number of the last order the
/// owner phone has, as JSON.
pub fn orders_request_body(since: u64) -> String {
    format!("{{\"since\":{since}}}")
}

impl SignedRequest {
    /// A number used once from the kernel's bytes.
    pub fn nonce_from(rng: &mut dyn KernelRandom) -> Result<[u8; NONCE_BYTES], WebOrderRefusal> {
        let bytes: [u8; KERNEL_BYTES] = rng.read32().ok_or(WebOrderRefusal::Random)?;
        bytes
            .get(..NONCE_BYTES)
            .and_then(|s| s.try_into().ok())
            .ok_or(WebOrderRefusal::Random)
    }

    /// WO-20: the HMAC of a request. The method is upper-case ASCII letters,
    /// the route starts with a slash and holds no line feed.
    pub fn make(
        secret: &[u8; SECRET_BYTES],
        method: &str,
        route: &str,
        time: u64,
        nonce: [u8; NONCE_BYTES],
        body: &[u8],
    ) -> Result<Self, WebOrderRefusal> {
        let bad = WebOrderRefusal::Request;
        if method.is_empty()
            || !method.bytes().all(|b| b.is_ascii_uppercase())
            || !route.starts_with('/')
            || !route.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(bad);
        }
        let message = format!(
            "{TAG}\n{method}\n{route}\n{time}\n{}\n{}",
            hex(&nonce),
            hex(&sha256::Hash::hash(body).to_byte_array())
        );
        let mut engine = hmac::HmacEngine::<sha256::Hash>::new(secret);
        engine.input(message.as_bytes());
        let mac = hmac::Hmac::<sha256::Hash>::from_engine(engine).to_byte_array();
        Ok(Self { time, nonce, mac })
    }

    /// The value of [`HEADER_TIME`].
    pub fn time_text(&self) -> String {
        self.time.to_string()
    }

    /// The value of [`HEADER_NONCE`].
    pub fn nonce_text(&self) -> String {
        hex(&self.nonce)
    }

    /// The value of [`HEADER_MAC`].
    pub fn mac_text(&self) -> String {
        hex(&self.mac)
    }
}
