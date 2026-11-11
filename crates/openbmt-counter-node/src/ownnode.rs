//! The counter's own-node setting (`docs/COUNTER-RULES.md`, CR-100 to
//! CR-102).
//!
//! The address of the merchant's own node is a setting of this counter, not
//! of the shop: it is a hint of where to ask, and what makes the node
//! trusted is the name itself (NR-98). [`own_node_setting`] reads the text a
//! manager typed and gives the node the driver is started with and the
//! canonical text the app keeps; it refuses what the driver would refuse, and
//! refuses a turn-on while the counter does not use Tor, with words for each.

use std::fmt;

use bitcoin::Network;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_net_io::{OwnNode, OwnNodeHost, OwnNodeRefused};

/// Why an own node was not set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnNodeSettingRefused {
    /// CR-101: the address is not a Tor address: a home-network, loopback or
    /// clear-net address, or a name of another kind. Such an address cannot
    /// prove which machine answers (NR-99).
    NotTor,
    /// CR-100: the text ends in `.onion` but is not a Tor v3 name (wrong
    /// length, characters, checksum or version), or is empty.
    BadName,
    /// CR-100: the port is not a number from 1 to 65535.
    BadPort,
    /// CR-102: the own node is reached through Tor and the counter has no
    /// Tor proxy (CR-87).
    NeedsTor,
}

impl OwnNodeSettingRefused {
    /// The rule this refusal belongs to.
    pub fn rule(self) -> &'static str {
        match self {
            Self::NotTor => "CR-101",
            Self::BadName | Self::BadPort => "CR-100",
            Self::NeedsTor => "CR-102",
        }
    }
}

impl fmt::Display for OwnNodeSettingRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotTor => write!(
                f,
                "CR-101: a home-network or clear-net address cannot be the own node"
            ),
            Self::BadName => write!(f, "CR-100: not a Tor v3 address"),
            Self::BadPort => write!(f, "CR-100: the port is not from 1 to 65535"),
            Self::NeedsTor => write!(
                f,
                "CR-102: the own node needs Tor and the counter has no Tor proxy"
            ),
        }
    }
}

impl std::error::Error for OwnNodeSettingRefused {}

impl OwnNodeSettingRefused {
    /// What the manager is told.
    pub fn words(&self) -> Words {
        let (heading, what, next) = match self {
            Self::NotTor => (
                "Not a Tor address",
                "A home-network address cannot prove which machine answers, so it cannot be your own node.",
                "Use your node's Tor address, which ends in .onion. Your node's own app shows it, often under its connection details.",
            ),
            Self::BadName => (
                "Tor address not accepted",
                "That is not a Tor address: it is 56 small letters and digits, then .onion.",
                "Copy the address again from your node, then try again.",
            ),
            Self::BadPort => (
                "Port not accepted",
                "The port after the colon must be a number from 1 to 65535.",
                "Fix the port, or leave it out to use the usual one.",
            ),
            Self::NeedsTor => (
                "Turn on Orbot first",
                "Your node is reached through Tor, and this counter is not using Tor.",
                "Turn on Orbot and Tor in the shop's settings, then add your node again.",
            ),
        };
        Words::new(Tone::Ended, heading, what, Who::Manager, next).code(self.rule())
    }
}

/// One value of every refusal, with its name, for SC-01.
pub fn every_setting_refusal() -> [(&'static str, OwnNodeSettingRefused); 4] {
    [
        ("NotTor", OwnNodeSettingRefused::NotTor),
        ("BadName", OwnNodeSettingRefused::BadName),
        ("BadPort", OwnNodeSettingRefused::BadPort),
        ("NeedsTor", OwnNodeSettingRefused::NeedsTor),
    ]
}

/// An own node that was accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnNodeSetting {
    /// What the driver is started with.
    pub node: OwnNode,
    /// The canonical text, `<56 characters>.onion:<port>`, which the app
    /// keeps and hands back at every start.
    pub text: String,
}

/// The port the network's Bitcoin Core listens on, when the text has none.
pub fn default_port(network: Network) -> u16 {
    match network {
        Network::Signet => 38_333,
        Network::Regtest => 18_444,
        _ => 8_333,
    }
}

/// CR-100 to CR-102: the own node from the text a manager typed. `tor_proxy`
/// says whether the counter has a Tor proxy (CR-87); without one the own
/// node is refused, because it is reached through Tor alone.
pub fn own_node_setting(
    text: &str,
    network: Network,
    tor_proxy: bool,
) -> Result<OwnNodeSetting, OwnNodeSettingRefused> {
    let text = text.trim();
    let host = text.split_once(':').map_or(text, |(host, _)| host);
    if !text.is_empty() && !host.ends_with(".onion") {
        return Err(OwnNodeSettingRefused::NotTor);
    }
    let node = OwnNode::parse(text, default_port(network)).map_err(|e| match e {
        OwnNodeRefused::NotOnion => OwnNodeSettingRefused::BadName,
        OwnNodeRefused::BadPort => OwnNodeSettingRefused::BadPort,
    })?;
    if !tor_proxy {
        return Err(OwnNodeSettingRefused::NeedsTor);
    }
    let OwnNodeHost::Onion(name) = node.host() else {
        // `parse` makes only Tor nodes; a node of another kind is not one
        // this setting can keep.
        return Err(OwnNodeSettingRefused::NotTor);
    };
    let text = format!("{name}:{}", node.port());
    Ok(OwnNodeSetting { node, text })
}
