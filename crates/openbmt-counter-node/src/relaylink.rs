//! The counter's link to the owner, started from the relay the counter
//! keeps with its pairing (`docs/COUNTER-RULES.md`, CR-134 to CR-136).
//!
//! The relay comes from the owner's bundle or from the owner's signed relay
//! for counters ([`openbmt_core::relay`]) and is kept in the pairing record.
//! It is a hint and adds no authority (`docs/LINK-RULES.md`, LK-09): the
//! owner's key is the pinned root, the relay's key is checked by the relay's
//! hello (LK-32), and a wrong relay only fails to deliver.
//!
//! [`link_route`] says how the relay is reached:
//!
//! - an onion route goes through the proxy, and needs one;
//! - a direct route goes direct, unless the verified settings have Tor on
//!   (CR-87), when it goes through the proxy as an address.
//!
//! [`start_link`] makes the courier on the counter's data folder, with the
//! owner as the one device it talks to, and the link with its default
//! intervals. [`LinkLine`] is what the counter's screen says about it.

use std::net::SocketAddr;
use std::path::Path;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::relay::{RelayHint, RelayRoute};
use openbmt_courier::{Courier, CourierConfig, FirstStart};
use openbmt_link::{ClientConfig, Role, Route};

use crate::link::{CounterLink, CounterLinkConfig};

/// The two files of the link's numbers (LK-53), in the counter's data
/// folder.
const NUMBER_FILES: [&str; 2] = ["link-inbox", "link-outbox"];

/// Why the counter did not start its link. The counter itself starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkNotStarted {
    /// CR-134: the relay is an onion service, or the settings have Tor on,
    /// and the start has no proxy.
    NeedsProxy,
    /// CR-134: the relay's key is not a key. A hint made by this library
    /// never has this.
    BadKey,
    /// CR-134: the link's files could not be opened.
    Files(String),
}

impl std::fmt::Display for LinkNotStarted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedsProxy => write!(
                f,
                "CR-134: the relay is reached through Tor and the start has no proxy"
            ),
            Self::BadKey => write!(f, "CR-134: the relay's key is not a key"),
            Self::Files(e) => write!(f, "CR-134: the link's files did not open: {e}"),
        }
    }
}

impl std::error::Error for LinkNotStarted {}

/// CR-134: how the counter reaches `hint`. `proxy` is the Tor proxy the
/// start was given, and `tor_on` is the verified settings' Tor switch
/// (CR-87).
pub fn link_route(
    hint: &RelayHint,
    proxy: Option<SocketAddr>,
    tor_on: bool,
) -> Result<Route, LinkNotStarted> {
    match &hint.route {
        RelayRoute::Direct(addr) if !tor_on => Ok(Route::Direct(*addr)),
        RelayRoute::Direct(addr) => Ok(Route::Proxied {
            proxy: proxy.ok_or(LinkNotStarted::NeedsProxy)?,
            addr: *addr,
        }),
        RelayRoute::Onion { name, port } => Ok(Route::Onion {
            proxy: proxy.ok_or(LinkNotStarted::NeedsProxy)?,
            host: format!("{name}.onion"),
            port: *port,
        }),
    }
}

/// CR-134: the link to the owner through `hint`, on the data folder `dir`.
/// The start is a first start (LK-53) when neither number file is there; any
/// other start needs both. `identity` is the counter's own and `owner` the
/// owner root it pinned.
pub fn start_link(
    dir: &Path,
    identity: Identity,
    owner: DeviceId,
    hint: &RelayHint,
    proxy: Option<SocketAddr>,
    tor_on: bool,
) -> Result<CounterLink, LinkNotStarted> {
    let route = link_route(hint, proxy, tor_on)?;
    let relay = hint.device().ok_or(LinkNotStarted::BadKey)?;
    let first = if NUMBER_FILES.iter().any(|f| dir.join(f).exists()) {
        FirstStart::No
    } else {
        FirstStart::Yes
    };
    let courier = Courier::start(
        CourierConfig {
            dir: dir.to_path_buf(),
            identity,
            role: Role::Counter,
            peers: std::collections::BTreeMap::from([(owner, Role::Owner)]),
            route,
            relay,
            client: ClientConfig::default(),
        },
        first,
    )
    .map_err(|e| LinkNotStarted::Files(e.to_string()))?;
    Ok(CounterLink::new(
        courier,
        owner,
        CounterLinkConfig::default(),
    ))
}

/// What the counter's screen says about the link (CR-136). Its words are in
/// [`crate::say`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkLine {
    /// No call to the relay has worked or failed yet.
    Connecting,
    /// The last call to the relay worked.
    Connected,
    /// The relay could not be reached, since the time given (when the
    /// counter first found out, seconds since 1970).
    NotReachable {
        /// When the failures began, if known.
        since: Option<u64>,
    },
    /// The relay answered and refused the counter.
    Refused,
    /// The relay is reached through Tor and the start has no proxy.
    NeedsTor,
    /// The link's files did not open, so there is no link.
    NotStarted,
}

impl From<&LinkNotStarted> for LinkLine {
    fn from(e: &LinkNotStarted) -> Self {
        match e {
            LinkNotStarted::NeedsProxy => Self::NeedsTor,
            LinkNotStarted::BadKey | LinkNotStarted::Files(_) => Self::NotStarted,
        }
    }
}
