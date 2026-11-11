//! What the counter holds to reach the shop's node: where the node is, the
//! certificate it must show, and the one credential that is limited to
//! invoices (LN-04 to LN-12).
//!
//! Reading a credential is never enough to know what it can do: the node
//! software enforces the limit, so every setup also probes it (`connect`).
//! Here a credential is only refused when its own text shows it is wider than
//! invoices, or when it is not in a form this crate understands.

use std::fmt;

use bitcoin::hashes::{sha256, Hash};

use crate::error::CredentialRefused;
use crate::text::{hex, plain, unbase64, unhex, Alphabet};

/// The longest connect string or rune the crate reads, in bytes.
pub const MAX_CREDENTIAL_BYTES: usize = 8 * 1024;
/// The longest macaroon the crate takes, in bytes. A macaroon baked for two
/// permissions is a few hundred bytes.
pub const MAX_MACAROON_BYTES: usize = 2048;
/// The longest certificate the crate takes, in bytes.
pub const MAX_CERTIFICATE_BYTES: usize = 8192;

/// The two methods of Core Lightning that the crate uses (LN-08, LN-10).
pub const CLN_METHODS: [&str; 2] = ["invoice", "listinvoices"];

/// The certificate the node must show (LN-12). The app's TLS code takes the
/// place of the usual check against certificate authorities with
/// [`CertPin::accepts`]: a node that shows anything else is not connected to.
#[derive(Clone, PartialEq, Eq)]
pub enum CertPin {
    /// The DER bytes of the certificate itself.
    Der(Vec<u8>),
    /// The SHA-256 of the DER bytes of the certificate.
    Sha256([u8; 32]),
}

impl fmt::Debug for CertPin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CertPin(sha256 {})", hex(&self.sha256()))
    }
}

/// Whether `der` is one DER sequence, as every certificate is: the tag, a
/// length, and exactly that many bytes.
fn is_der_sequence(der: &[u8]) -> bool {
    if der.len() < 64 || der.len() > MAX_CERTIFICATE_BYTES {
        return false;
    }
    let (Some(&tag), Some(&first)) = (der.first(), der.get(1)) else {
        return false;
    };
    if tag != 0x30 {
        return false;
    }
    let (header, len) = match first {
        0..=0x7f => (2usize, usize::from(first)),
        0x81 => match der.get(2) {
            Some(&l) if l >= 0x80 => (3, usize::from(l)),
            _ => return false,
        },
        0x82 => match (der.get(2), der.get(3)) {
            (Some(&h), Some(&l)) if h != 0 => (4, usize::from(h) << 8 | usize::from(l)),
            _ => return false,
        },
        _ => return false,
    };
    header + len == der.len()
}

impl CertPin {
    /// Pin the certificate with these DER bytes.
    pub fn from_der(der: &[u8]) -> Result<Self, CredentialRefused> {
        if is_der_sequence(der) {
            Ok(Self::Der(der.to_vec()))
        } else {
            Err(CredentialRefused::BadCertificate)
        }
    }

    /// Pin the one certificate in a PEM file, as the node writes it
    /// (`tls.cert` of LND, the certificate of clnrest).
    pub fn from_pem(pem: &str) -> Result<Self, CredentialRefused> {
        const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
        const END: &str = "-----END CERTIFICATE-----";
        if pem.len() > MAX_CERTIFICATE_BYTES * 2 {
            return Err(CredentialRefused::BadCertificate);
        }
        let start = pem.find(BEGIN).ok_or(CredentialRefused::BadCertificate)?;
        let rest = pem.get(start + BEGIN.len()..).unwrap_or("");
        let end = rest.find(END).ok_or(CredentialRefused::BadCertificate)?;
        // Only one certificate: a chain is not what the node's own file is.
        if rest.get(end + END.len()..).unwrap_or("").contains(BEGIN) {
            return Err(CredentialRefused::BadCertificate);
        }
        let body: String = rest
            .get(..end)
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let der = unbase64(&body, Alphabet::Standard).ok_or(CredentialRefused::BadCertificate)?;
        Self::from_der(&der)
    }

    /// Pin a certificate by its SHA-256 fingerprint: 64 hex digits, which
    /// may be written in pairs with colons, as `openssl x509 -fingerprint
    /// -sha256` writes them.
    pub fn from_sha256_hex(text: &str) -> Result<Self, CredentialRefused> {
        let digits: String = text.chars().filter(|c| *c != ':').collect();
        let bytes = unhex(&digits)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            .ok_or(CredentialRefused::BadFingerprint)?;
        Ok(Self::Sha256(bytes))
    }

    /// The SHA-256 of the pinned certificate.
    pub fn sha256(&self) -> [u8; 32] {
        match self {
            Self::Der(der) => sha256::Hash::hash(der).to_byte_array(),
            Self::Sha256(d) => *d,
        }
    }

    /// Whether the certificate the node showed, as DER bytes, is the pinned
    /// one. The app's TLS code connects only when this is true, and checks
    /// nothing else about the certificate: a pin replaces the authorities.
    pub fn accepts(&self, presented_der: &[u8]) -> bool {
        match self {
            Self::Der(der) => der == presented_der,
            Self::Sha256(d) => sha256::Hash::hash(presented_der).to_byte_array() == *d,
        }
    }
}

/// Whether `host` is a name or an address: letters, digits, dots and
/// hyphens in labels of 1 to 63, or an address of letters, digits, colons and
/// dots in square brackets.
fn is_host(host: &str) -> bool {
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return (2..=45).contains(&inner.len())
            && inner.contains(':')
            && inner
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
    }
    host.len() <= 253
        && host
            .split('.')
            .all(|l| plain(l, 63, "-") && !l.starts_with('-') && !l.ends_with('-'))
}

/// Where the node is and the certificate it must show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    host: String,
    port: u16,
    pin: CertPin,
}

impl Endpoint {
    /// From `host:port` (an address with colons goes in square brackets) and
    /// the pin.
    pub fn parse(host_port: &str, pin: CertPin) -> Result<Self, CredentialRefused> {
        let (host, port) = host_port
            .rsplit_once(':')
            .ok_or(CredentialRefused::BadAddress)?;
        if !is_host(host) || !plain(port, 5, "") || port.starts_with('0') {
            return Err(CredentialRefused::BadAddress);
        }
        let port: u16 = port.parse().map_err(|_| CredentialRefused::BadAddress)?;
        Ok(Self {
            host: host.to_string(),
            port,
            pin,
        })
    }

    /// The host name or address (an address with colons is in brackets).
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The certificate the node must show: the app's TLS code takes it
    /// through [`CertPin::accepts`]. The crate does no TLS itself, and the
    /// requests are for `https` only.
    pub fn pin(&self) -> &CertPin {
        &self.pin
    }

    /// `host:port`, as the app opens a connection to it.
    pub fn host_port(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// A macaroon baked for LND with `invoices:read` and `invoices:write` only
/// (LN-05), and where the node is.
#[derive(Clone, PartialEq, Eq)]
pub struct LndCredential {
    endpoint: Endpoint,
    macaroon: Vec<u8>,
}

impl fmt::Debug for LndCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LndCredential")
            .field("endpoint", &self.endpoint)
            .field("macaroon", &"<held, not shown>")
            .finish()
    }
}

impl LndCredential {
    /// From the bytes of the macaroon file. What the macaroon may do cannot
    /// be read from its bytes (the format is not documented for clients), so
    /// only its shape is checked here and `connect` probes the rest (LN-06).
    pub fn new(endpoint: Endpoint, macaroon: &[u8]) -> Result<Self, CredentialRefused> {
        // A macaroon in the binary form of version 2 begins with the byte 2.
        if macaroon.is_empty()
            || macaroon.len() > MAX_MACAROON_BYTES
            || macaroon.first() != Some(&2)
        {
            return Err(CredentialRefused::BadMacaroon);
        }
        Ok(Self {
            endpoint,
            macaroon: macaroon.to_vec(),
        })
    }

    /// From the `lndconnect://` address that Umbrel and Start9 show (LN-11):
    /// `lndconnect://host:port?cert=<base64url>&macaroon=<base64url>`. The
    /// certificate is required, because it is pinned (LN-12). Any other
    /// parameter, a parameter twice or a missing one is refused.
    pub fn from_lndconnect(uri: &str) -> Result<Self, CredentialRefused> {
        if uri.len() > MAX_CREDENTIAL_BYTES {
            return Err(CredentialRefused::BadConnectString);
        }
        let rest = uri
            .strip_prefix("lndconnect://")
            .ok_or(CredentialRefused::BadConnectString)?;
        let (authority, query) = rest
            .split_once('?')
            .ok_or(CredentialRefused::BadConnectString)?;
        let (mut cert, mut macaroon) = (None, None);
        for pair in query.split('&') {
            let (key, value) = pair
                .split_once('=')
                .ok_or(CredentialRefused::BadConnectString)?;
            let slot = match key {
                "cert" => &mut cert,
                "macaroon" => &mut macaroon,
                _ => return Err(CredentialRefused::BadConnectString),
            };
            if slot.replace(value).is_some() {
                return Err(CredentialRefused::BadConnectString);
            }
        }
        let cert = cert.ok_or(CredentialRefused::NoCertificate)?;
        let macaroon = macaroon.ok_or(CredentialRefused::BadMacaroon)?;
        let der = unbase64(cert, Alphabet::Url).ok_or(CredentialRefused::BadCertificate)?;
        let pin = CertPin::from_der(&der)?;
        let macaroon = unbase64(macaroon, Alphabet::Url).ok_or(CredentialRefused::BadMacaroon)?;
        Self::new(Endpoint::parse(authority, pin)?, &macaroon)
    }

    /// Where the node is.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The value of the `Grpc-Metadata-macaroon` header: the macaroon in hex
    /// (LN-07).
    pub(crate) fn header_value(&self) -> String {
        hex(&self.macaroon)
    }
}

/// A rune for Core Lightning, limited to invoices (LN-08), and where the
/// node is.
#[derive(Clone, PartialEq, Eq)]
pub struct ClnCredential {
    endpoint: Endpoint,
    rune: String,
}

impl fmt::Debug for ClnCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClnCredential")
            .field("endpoint", &self.endpoint)
            .field("rune", &"<held, not shown>")
            .finish()
    }
}

impl ClnCredential {
    /// From the rune as Core Lightning writes it, after reading it in the
    /// clear and refusing it unless it limits the method to the invoice ones
    /// (LN-08).
    pub fn new(endpoint: Endpoint, rune: &str) -> Result<Self, CredentialRefused> {
        check_rune(rune)?;
        Ok(Self {
            endpoint,
            rune: rune.to_string(),
        })
    }

    /// Where the node is.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The value of the `Rune` header (LN-10).
    pub(crate) fn header_value(&self) -> &str {
        &self.rune
    }
}

/// What a person sets up: a node of one of the two kinds the crate speaks to
/// (LN-01). Eclair is not one of them (LN-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// LND, over its REST interface.
    Lnd(LndCredential),
    /// Core Lightning, over clnrest.
    Cln(ClnCredential),
}

impl Credential {
    /// Where the node is.
    pub fn endpoint(&self) -> &Endpoint {
        match self {
            Self::Lnd(c) => c.endpoint(),
            Self::Cln(c) => c.endpoint(),
        }
    }
}

// The grammar of a rune, from the Core Lightning documentation of `createrune`
// (https://docs.corelightning.org/reference/createrune) and the runes
// repository (https://github.com/rustyrussell/runes): the rune is the
// base64url of a 32-byte hash and then the restrictions, written in the
// clear, joined by `&`. A restriction is alternatives joined by `|`; an
// alternative is a field name, one operator of `= / ^ $ ~ < > { } # !` and a
// value, in which `\` escapes `\`, `|` and `&`.

const OPERATORS: &str = "=/^$~<>{}#!";

struct Alternative {
    field: String,
    op: char,
    value: String,
}

/// Cut `text` at every `sep` that is not escaped, keeping the escapes.
fn split_unescaped(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut escaped) = (0, false);
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == sep {
            parts.push(text.get(start..i).unwrap_or(""));
            start = i + c.len_utf8();
        }
    }
    parts.push(text.get(start..).unwrap_or(""));
    parts
}

fn unescape(value: &str) -> Result<String, CredentialRefused> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(e @ ('\\' | '|' | '&')) => out.push(e),
            _ => return Err(CredentialRefused::RuneSyntax("an escape")),
        }
    }
    Ok(out)
}

fn alternative(text: &str) -> Result<Alternative, CredentialRefused> {
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .ok_or(CredentialRefused::RuneSyntax(
            "an alternative without an operator",
        ))?;
    let (field, rest) = text.split_at(end);
    let mut chars = rest.chars();
    let op = chars
        .next()
        .filter(|c| OPERATORS.contains(*c))
        .ok_or(CredentialRefused::RuneSyntax("an operator"))?;
    Ok(Alternative {
        field: field.to_string(),
        op,
        value: unescape(chars.as_str())?,
    })
}

/// LN-08: read the restrictions of a rune and refuse it unless it limits the
/// method to a subset of `invoice` and `listinvoices` that leaves both.
///
/// Restrictions are all required (`&`), and a restriction is met by any one
/// of its alternatives (`|`), so a rune can only be as wide as its widest
/// method restriction allows. A restriction that names the method must be
/// made of `method=` alternatives and nothing else; a restriction that
/// names another method, or the method with another operator, or the method
/// together with another field, is refused whole, even when another
/// restriction would narrow it again. Restrictions that do not name the
/// method only narrow the rune further and are accepted when they are well
/// formed.
pub fn check_rune(rune: &str) -> Result<(), CredentialRefused> {
    if rune.len() > MAX_CREDENTIAL_BYTES {
        return Err(CredentialRefused::RuneNotBase64);
    }
    let bytes = unbase64(rune, Alphabet::Url).ok_or(CredentialRefused::RuneNotBase64)?;
    let restrictions = bytes.get(32..).ok_or(CredentialRefused::RuneNotBase64)?;
    if restrictions.is_empty() {
        return Err(CredentialRefused::RuneUnrestricted);
    }
    let text = std::str::from_utf8(restrictions).map_err(|_| CredentialRefused::RuneNotText)?;
    // What every restriction of the method allows together, if there is one.
    let mut allowed: Option<Vec<String>> = None;
    for group in split_unescaped(text, '&') {
        let alternatives = split_unescaped(group, '|')
            .into_iter()
            .map(alternative)
            .collect::<Result<Vec<_>, _>>()?;
        if !alternatives.iter().any(|a| a.field == "method") {
            continue;
        }
        let mut methods = Vec::new();
        for a in &alternatives {
            if a.field != "method" || a.op != '=' {
                return Err(CredentialRefused::RuneMixesMethod);
            }
            if !CLN_METHODS.contains(&a.value.as_str()) {
                return Err(CredentialRefused::RuneAllowsMethod(a.value.clone()));
            }
            methods.push(a.value.clone());
        }
        allowed = Some(match allowed {
            None => methods,
            Some(before) => before.into_iter().filter(|m| methods.contains(m)).collect(),
        });
    }
    match allowed {
        Some(m) if CLN_METHODS.iter().all(|need| m.iter().any(|x| x == need)) => Ok(()),
        _ => Err(CredentialRefused::RuneLacksMethod),
    }
}
