//! Turning Shop Lightning on, changing it and turning it off from the owner
//! phone (`docs/SHOP-LIGHTNING-RULES.md` SL-71, SL-73, SL-76, SL-77,
//! `docs/APPROVAL-RULES.md` AP-13).
//!
//! An edit of the field is applied to the settings held by
//! [`Owner::settings_preview`](crate::Owner::settings_preview), checked, and
//! sorted as any other change of the settings is: the queue gives a change
//! that loosens to the Key app, and signs one that tightens with the owner's
//! key alone. [`openbmt_core::approval::classify`] gives its class.
//!
//! No sentence is written yet. Every text is the placeholder that names its
//! rule, `WORDS_NEEDED_SL<rule>_<NAME>_HEADING`, `_WHAT` and `_NEXT`, so that
//! the missing words show wherever the words will stand. As the web order
//! words do, they are not in the table of the library's other words, whose
//! checks would refuse a placeholder.

use std::fmt;
use std::net::SocketAddr;

use bitcoin::secp256k1::PublicKey;
use openbmt_core::identity::DeviceId;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::settings::{Settings, ShopLightning, LIGHTNING_NODE_BYTES};

use crate::error::Refusal;

/// The whole of a Shop Lightning block, as the turn-on steps gather it
/// (SL-71, SL-76). The two keys are text: the node's ID as the stick's code
/// gives it (SK-35), the provider's as 66 hexadecimal digits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopLightningSet {
    /// The Lightning counter, one of the shop's counters.
    pub counter: DeviceId,
    /// The node's ID: 66 lower-case hexadecimal digits that are a compressed
    /// public key.
    pub node_id: String,
    /// The Box that keeps the second copy. `None` takes the shop's Box.
    pub second_device: Option<DeviceId>,
    /// The provider's node ID, as 66 hexadecimal digits.
    pub provider_node_id: String,
    /// The provider's IP address and port, `203.0.113.7:9735` or
    /// `[2001:db8::1]:9735`.
    pub provider_addr: String,
    /// The most the node holds, in satoshis.
    pub cap_sats: u64,
    /// The most the shop pays for one opening, in satoshis.
    pub max_opening_fee_sats: u64,
}

/// A change of a Shop Lightning that is on: each field that is `None` is
/// kept (AP-13). The counter and the node's ID are not here: naming another
/// of them is a new turn-on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShopLightningChange {
    /// The cap, in satoshis.
    pub cap_sats: Option<u64>,
    /// The most for one opening, in satoshis.
    pub max_opening_fee_sats: Option<u64>,
    /// The provider's node ID (66 hexadecimal digits) and its IP address and
    /// port.
    pub provider: Option<(String, String)>,
    /// The Box that keeps the second copy.
    pub second_device: Option<DeviceId>,
}

/// What an edit does to Shop Lightning (SL-71).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShopLightningEdit {
    /// Take the field away: Shop Lightning is off, and the Lightning counter
    /// closes its channels (SL-77).
    Remove,
    /// Name the whole block: a turn-on, or a move to another counter or node.
    Set(ShopLightningSet),
    /// Change some fields of a block that is set.
    Change(ShopLightningChange),
}

/// Why an edit of Shop Lightning was refused. Each carries the rule it comes
/// from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopLightningRefusal {
    /// SL-76, AP-13: the shop uses its own wallet, and its owner phone has no
    /// way to carry a change of the settings that sends payments somewhere
    /// else or loosens a limit.
    OwnWallet,
    /// SL-73: the shop has no Box to keep the second copy.
    NoBox,
    /// SL-73: the Box named is not one of the shop's.
    BoxNotInShop,
    /// SL-71: the counter named is not one of the shop's counters.
    CounterNotInShop,
    /// SL-76, SK-35: the node's ID is not 66 lower-case hexadecimal digits
    /// that make a compressed public key.
    NodeId,
    /// SL-71: the provider's node ID is not 66 hexadecimal digits that make
    /// a compressed public key.
    ProviderKey,
    /// SL-71: the provider's address is not an IP address and a port from 1
    /// to 65535.
    ProviderAddress,
    /// SL-71: the cap is 0.
    CapZero,
    /// SL-71: the shop names a Lightning node or a swap provider, and Shop
    /// Lightning is never set with either.
    OtherLightning,
    /// SL-71: the edit changes a Shop Lightning that is off.
    NotOn,
}

impl ShopLightningRefusal {
    /// The rule this refusal is from, with its prefix.
    pub fn rule(self) -> &'static str {
        match self {
            Self::OwnWallet | Self::NodeId => "SL-76",
            Self::NoBox | Self::BoxNotInShop => "SL-73",
            Self::CounterNotInShop
            | Self::ProviderKey
            | Self::ProviderAddress
            | Self::CapZero
            | Self::OtherLightning
            | Self::NotOn => "SL-71",
        }
    }

    /// The name part of the placeholders of this refusal.
    fn name(self) -> &'static str {
        match self {
            Self::OwnWallet => "OWN_WALLET",
            Self::NoBox => "NO_BOX",
            Self::BoxNotInShop => "BOX_NOT_IN_SHOP",
            Self::CounterNotInShop => "COUNTER_NOT_IN_SHOP",
            Self::NodeId => "NODE_ID",
            Self::ProviderKey => "PROVIDER_KEY",
            Self::ProviderAddress => "PROVIDER_ADDRESS",
            Self::CapZero => "CAP_ZERO",
            Self::OtherLightning => "OTHER_LIGHTNING",
            Self::NotOn => "NOT_ON",
        }
    }

    /// Every refusal, for the checks.
    pub const ALL: [Self; 10] = [
        Self::OwnWallet,
        Self::NoBox,
        Self::BoxNotInShop,
        Self::CounterNotInShop,
        Self::NodeId,
        Self::ProviderKey,
        Self::ProviderAddress,
        Self::CapZero,
        Self::OtherLightning,
        Self::NotOn,
    ];

    /// What the owner is told: the placeholders of the refusal's rule.
    pub fn words(self) -> Words {
        let stem = format!(
            "WORDS_NEEDED_SL{}_{}",
            self.rule().trim_start_matches("SL-"),
            self.name()
        );
        Words::new(
            Tone::Stop,
            format!("{stem}_HEADING"),
            format!("{stem}_WHAT"),
            Who::Owner,
            format!("{stem}_NEXT"),
        )
    }
}

impl fmt::Display for ShopLightningRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Self::OwnWallet => {
                "this shop uses its own wallet, so no change of Shop Lightning is carried"
            }
            Self::NoBox => "the shop has no Box to keep the second copy",
            Self::BoxNotInShop => "the second device is not a Box of the shop",
            Self::CounterNotInShop => "the Lightning counter is not a counter of the shop",
            Self::NodeId => {
                "the node's ID is not 66 lower-case hexadecimal digits of a compressed key"
            }
            Self::ProviderKey => "the provider's node ID is not a compressed public key",
            Self::ProviderAddress => "the provider's address is not an IP address and a port",
            Self::CapZero => "the cap is 0",
            Self::OtherLightning => "the shop names a Lightning node or a swap provider",
            Self::NotOn => "Shop Lightning is off, so there is nothing to change",
        };
        write!(f, "{}: {why}", self.rule())
    }
}

impl From<ShopLightningRefusal> for Refusal {
    fn from(r: ShopLightningRefusal) -> Self {
        Self::ShopLightning(r)
    }
}

impl<E> From<ShopLightningRefusal> for crate::OwnerError<E> {
    fn from(r: ShopLightningRefusal) -> Self {
        Self::Refused(Refusal::ShopLightning(r))
    }
}

/// Every refusal with the name the checks list it by.
pub fn all_refusals() -> Vec<(String, Words)> {
    ShopLightningRefusal::ALL
        .iter()
        .map(|r| (format!("ShopLightning/{}", r.name()), r.words()))
        .collect()
}

fn hex_digits(text: &str) -> Option<[u8; LIGHTNING_NODE_BYTES]> {
    if text.len() != 2 * LIGHTNING_NODE_BYTES {
        return None;
    }
    let mut bytes = [0u8; LIGHTNING_NODE_BYTES];
    for (byte, pair) in bytes.iter_mut().zip(text.as_bytes().chunks(2)) {
        let digit = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        let [high, low] = pair else { return None };
        *byte = (digit(*high)? << 4) | digit(*low)?;
    }
    PublicKey::from_slice(&bytes).ok().map(|_| bytes)
}

/// SL-76, SK-35: the node's ID from the stick's code: exactly 66 lower-case
/// hexadecimal digits that parse as a compressed public key.
pub fn parse_node_id(text: &str) -> Result<[u8; LIGHTNING_NODE_BYTES], ShopLightningRefusal> {
    hex_digits(text).ok_or(ShopLightningRefusal::NodeId)
}

/// SL-71: the provider's node ID: 66 hexadecimal digits (capitals are read)
/// that parse as a compressed public key.
pub fn parse_provider_key(text: &str) -> Result<[u8; LIGHTNING_NODE_BYTES], ShopLightningRefusal> {
    hex_digits(&text.trim().to_ascii_lowercase()).ok_or(ShopLightningRefusal::ProviderKey)
}

/// SL-71, SL-32: the provider's IP address and port.
pub fn parse_provider_addr(text: &str) -> Result<SocketAddr, ShopLightningRefusal> {
    match text.trim().parse::<SocketAddr>() {
        Ok(addr) if addr.port() != 0 => Ok(addr),
        _ => Err(ShopLightningRefusal::ProviderAddress),
    }
}

/// The Box a block names: the one given, else the shop's first. The Box must
/// be one of the shop's.
fn second_device(s: &Settings, named: Option<DeviceId>) -> Result<DeviceId, ShopLightningRefusal> {
    match named {
        Some(b) if s.boxes.contains(&b) => Ok(b),
        Some(_) => Err(ShopLightningRefusal::BoxNotInShop),
        None => s
            .boxes
            .iter()
            .next()
            .copied()
            .ok_or(ShopLightningRefusal::NoBox),
    }
}

/// Apply `edit` to the Shop Lightning of `s` (SL-71). The settings are the
/// ones the edit is made on, with the edit's other fields already applied.
/// The block that results is checked as far as the owner's words go; the
/// settings' own check follows.
pub(crate) fn apply(
    s: &mut Settings,
    edit: &ShopLightningEdit,
) -> Result<(), ShopLightningRefusal> {
    use ShopLightningRefusal as R;
    match edit {
        ShopLightningEdit::Remove => s.shop_lightning = None,
        ShopLightningEdit::Set(set) => {
            if s.lightning_node.is_some() || s.swap.is_some() {
                return Err(R::OtherLightning);
            }
            if !s.counters.contains_key(&set.counter) {
                return Err(R::CounterNotInShop);
            }
            let node_id = parse_node_id(&set.node_id)?;
            let second_device = second_device(s, set.second_device)?;
            let provider_node_id = parse_provider_key(&set.provider_node_id)?;
            let provider_addr = parse_provider_addr(&set.provider_addr)?;
            if set.cap_sats == 0 {
                return Err(R::CapZero);
            }
            s.shop_lightning = Some(ShopLightning {
                counter: set.counter,
                node_id,
                second_device,
                provider_node_id,
                provider_addr,
                cap_sats: set.cap_sats,
                max_opening_fee_sats: set.max_opening_fee_sats,
            });
        }
        ShopLightningEdit::Change(change) => {
            let mut sl = s.shop_lightning.ok_or(R::NotOn)?;
            if let Some(cap) = change.cap_sats {
                if cap == 0 {
                    return Err(R::CapZero);
                }
                sl.cap_sats = cap;
            }
            if let Some(fee) = change.max_opening_fee_sats {
                sl.max_opening_fee_sats = fee;
            }
            if let Some((key, addr)) = &change.provider {
                sl.provider_node_id = parse_provider_key(key)?;
                sl.provider_addr = parse_provider_addr(addr)?;
            }
            if let Some(b) = change.second_device {
                sl.second_device = second_device(s, Some(b))?;
            }
            s.shop_lightning = Some(sl);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "shop_lightning_tests.rs"]
mod tests;
