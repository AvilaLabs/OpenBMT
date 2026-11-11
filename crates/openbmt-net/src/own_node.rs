//! Who the merchant's own node is (NR-98, NR-99).
//!
//! The own node is a Bitcoin Core node that the counter reaches over the
//! peer protocol. It counts as the own node only when it is reached at a Tor
//! v3 address that the owner set: the name of such an address is the node's
//! public key, so the proxy reaches that key or nothing, and no one on the
//! path can stand in for it. An [`OwnNode`] is the only way to say which
//! connection is the own node, and the only ways to make one are
//! [`OwnNode::onion`] and [`OwnNode::parse`], which take a v3 onion name and
//! nothing else.
//!
//! A LAN, loopback or clear-net address can be a fixed peer of the driver,
//! but it is never an [`OwnNode`]: nothing authenticates the node behind
//! it.
//!
//! One more constructor exists for a check against a node on the same
//! machine, `OwnNode::clear_net_for_tests`. It is compiled only with the
//! cargo feature `clear-own-node-for-tests`, which no program enables; the
//! only crate that turns it on is the driver's own tests, as a
//! dev-dependency (see `docs/NETWORK-RULES.md`, NR-99).

use std::fmt;
use std::net::SocketAddr;

use crate::onion::{onion_name, parse_onion_name, SUFFIX};

/// Where the own node is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnNodeHost {
    /// The host name of a Tor v3 address, reached through the proxy.
    Onion(String),
    /// A socket address on this machine. Only a test can make one
    /// ([`OwnNode`]); it is dialed without a proxy and is not
    /// authenticated.
    Clear(SocketAddr),
}

/// The merchant's own node: a Tor v3 address and a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnNode {
    host: OwnNodeHost,
    port: u16,
}

/// Why an own node was not made (NR-98).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnNodeRefused {
    /// The text is not the name of a Tor v3 address: a LAN, loopback or
    /// clear-net address, a name of another kind, a name that is not 56
    /// lower-case base32 characters, or one with a wrong checksum or version.
    NotOnion,
    /// The port is not a number from 1 to 65535.
    BadPort,
}

impl fmt::Display for OwnNodeRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotOnion => write!(
                f,
                "NR-98: the own node is only a Tor v3 address (56 characters and .onion)"
            ),
            Self::BadPort => write!(f, "NR-98: the own node's port is not from 1 to 65535"),
        }
    }
}

impl std::error::Error for OwnNodeRefused {}

impl OwnNode {
    /// NR-98: the own node at this Tor v3 host name (`<56 characters>.onion`)
    /// and port. A name that [`parse_onion_name`] does not accept is
    /// refused; so is port 0.
    pub fn onion(name: &str, port: u16) -> Result<Self, OwnNodeRefused> {
        if port == 0 {
            return Err(OwnNodeRefused::BadPort);
        }
        let key = parse_onion_name(name).ok_or(OwnNodeRefused::NotOnion)?;
        Ok(Self {
            host: OwnNodeHost::Onion(onion_name(&key)),
            port,
        })
    }

    /// NR-98: the own node from the text the owner typed: a Tor v3 host name,
    /// with a `:port` after it or, without one, `default_port`. Anything
    /// else is refused, whatever it looks like.
    pub fn parse(text: &str, default_port: u16) -> Result<Self, OwnNodeRefused> {
        let text = text.trim();
        match text.split_once(':') {
            Some((name, port)) if name.ends_with(SUFFIX) => {
                let port = port.parse::<u16>().map_err(|_| OwnNodeRefused::BadPort)?;
                Self::onion(name, port)
            }
            Some(_) => Err(OwnNodeRefused::NotOnion),
            None => Self::onion(text, default_port),
        }
    }

    /// NR-99: a node on this machine, for the check of the driver against
    /// Bitcoin Core on regtest. It is not authenticated and is reached
    /// without a proxy. Compiled only with the feature
    /// `clear-own-node-for-tests`.
    #[cfg(feature = "clear-own-node-for-tests")]
    pub fn clear_net_for_tests(addr: SocketAddr) -> Self {
        Self {
            port: addr.port(),
            host: OwnNodeHost::Clear(addr),
        }
    }

    /// Where the node is reached.
    pub fn host(&self) -> &OwnNodeHost {
        &self.host
    }

    /// The port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Whether the address authenticates the node: true for a Tor v3
    /// address, false for the address of a test.
    pub fn is_authenticated(&self) -> bool {
        matches!(self.host, OwnNodeHost::Onion(_))
    }
}
