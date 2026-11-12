//! A swap's state file (SW-50 to SW-54): what a claim needs after a restart.
//!
//! The file holds the preimage and the claim key in the clear. Whoever keeps
//! it keeps it sealed, as the app keeps its other state under the PIN lock.
//!
//! Layout, all numbers big-endian: magic `OBM%`, version 1, network (1),
//! key order (1), phase (1), preimage (32), claim key (32), refund key (33),
//! timeout (4), on-chain satoshis (8), invoice satoshis (8), invoice expiry
//! (8), id (1 length byte), invoice (2 length bytes), receive address (1
//! length byte), then for the phase claimed the claim transaction's id (32).

use std::fmt;
use std::str::FromStr;

use bitcoin::hashes::Hash;
use bitcoin::secp256k1::PublicKey;
use bitcoin::{Address, Network, Txid};
use openbmt_lightning::bolt11;

use crate::error::Refused;
use crate::keys::Secrets;
use crate::reply::Swap;
use crate::tree::{KeyOrder, SwapTree};
use crate::{MAX_INVOICE_SATS, MIN_ONCHAIN_SATS};

/// SW-50: the first four bytes of the file.
pub const MAGIC_SWAP_STATE: [u8; 4] = *b"OBM%";
/// SW-50: the version of the layout.
pub const STATE_VERSION: u8 = 1;
/// The most bytes of the invoice the file keeps.
const MAX_INVOICE_BYTES: usize = bolt11::MAX_BOLT11_CHARS;
/// The longest text of an address (a bech32m address of 62 characters).
const MAX_ADDRESS_BYTES: usize = 100;
/// The longest id (SW-12).
const MAX_ID_BYTES: usize = 64;

/// SW-53: how far a swap has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The claim has not been built and sent.
    Waiting,
    /// The claim was built and sent, with this transaction id.
    Claimed(Txid),
}

/// A swap, as it is kept between runs.
#[derive(Clone, PartialEq, Eq)]
pub struct SwapState {
    id: String,
    network: Network,
    secrets: Secrets,
    refund_key: PublicKey,
    order: KeyOrder,
    timeout: u32,
    onchain_sats: u64,
    invoice_sats: u64,
    invoice_expires_at: u64,
    invoice: String,
    destination: Address,
    phase: Phase,
}

impl fmt::Debug for SwapState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwapState")
            .field("id", &self.id)
            .field("network", &self.network)
            .field("timeout", &self.timeout)
            .field("phase", &self.phase)
            .finish_non_exhaustive()
    }
}

fn network_byte(n: Network) -> Option<u8> {
    match n {
        Network::Bitcoin => Some(0),
        Network::Testnet => Some(1),
        Network::Signet => Some(2),
        Network::Regtest => Some(3),
        _ => None,
    }
}

fn network_from(b: u8) -> Option<Network> {
    match b {
        0 => Some(Network::Bitcoin),
        1 => Some(Network::Testnet),
        2 => Some(Network::Signet),
        3 => Some(Network::Regtest),
        _ => None,
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A reader of exact widths.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Refused> {
        let (head, rest) = self
            .0
            .split_at_checked(n)
            .ok_or(Refused::BadState("too short"))?;
        self.0 = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Refused> {
        <[u8; N]>::try_from(self.take(N)?).map_err(|_| Refused::BadState("too short"))
    }

    fn byte(&mut self) -> Result<u8, Refused> {
        Ok(self.array::<1>()?[0])
    }

    fn text(&mut self, n: usize) -> Result<String, Refused> {
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| Refused::BadState("not text"))
    }
}

impl SwapState {
    /// A new state in the phase waiting, from a swap that passed `check`,
    /// the secrets it was made with and the receive address the claim pays.
    pub fn new(
        swap: &Swap,
        secrets: &Secrets,
        network: Network,
        destination: &Address,
    ) -> Result<Self, Refused> {
        if !destination.as_unchecked().is_valid_for_network(network) {
            return Err(Refused::DestinationNetwork);
        }
        let state = Self {
            id: swap.id.clone(),
            network,
            secrets: secrets.clone(),
            refund_key: swap.refund_key,
            order: swap.order,
            timeout: swap.timeout,
            onchain_sats: swap.onchain_sats,
            invoice_sats: swap.invoice_sats,
            invoice_expires_at: swap.invoice_expires_at,
            invoice: swap.invoice.clone(),
            destination: destination.clone(),
            phase: Phase::Waiting,
        };
        state.validate()?;
        Ok(state)
    }

    /// SW-52: that what the state holds agrees with itself.
    fn validate(&self) -> Result<(), Refused> {
        if network_byte(self.network).is_none() {
            return Err(Refused::BadState("network"));
        }
        if !valid_id(&self.id) {
            return Err(Refused::BadState("id"));
        }
        if self.refund_key == self.secrets.claim_public() {
            return Err(Refused::BadState("refund key"));
        }
        if self.timeout >= 500_000_000 || self.timeout == 0 {
            return Err(Refused::BadState("timeout"));
        }
        if self.onchain_sats < MIN_ONCHAIN_SATS
            || self.onchain_sats > self.invoice_sats
            || self.invoice_sats > MAX_INVOICE_SATS
        {
            return Err(Refused::BadState("amount"));
        }
        if self.invoice.len() > MAX_INVOICE_BYTES || self.invoice != self.invoice.to_lowercase() {
            return Err(Refused::BadState("invoice"));
        }
        let invoice = bolt11::decode(&self.invoice).map_err(|_| Refused::BadState("invoice"))?;
        if invoice.payment_hash != self.secrets.preimage_hash()
            || bolt11::network_prefix(self.network) != Some(invoice.network)
            || invoice.amount_msat != self.invoice_sats.checked_mul(1000)
        {
            return Err(Refused::BadState("invoice"));
        }
        if invoice.timestamp.checked_add(invoice.expiry_secs) != Some(self.invoice_expires_at) {
            return Err(Refused::BadState("invoice expiry"));
        }
        if !self
            .destination
            .as_unchecked()
            .is_valid_for_network(self.network)
            || self.destination.to_string().len() > MAX_ADDRESS_BYTES
        {
            return Err(Refused::BadState("receive address"));
        }
        Ok(())
    }

    /// The service's id for the swap.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The shop's network.
    pub fn network(&self) -> Network {
        self.network
    }

    /// The preimage and the claim key.
    pub fn secrets(&self) -> &Secrets {
        &self.secrets
    }

    /// The block height after which the service can take the lockup back.
    pub fn timeout(&self) -> u32 {
        self.timeout
    }

    /// The amount the service promised to lock.
    pub fn onchain_sats(&self) -> u64 {
        self.onchain_sats
    }

    /// The invoice for the customer to pay.
    pub fn invoice(&self) -> &str {
        &self.invoice
    }

    /// When the invoice runs out, in seconds since 1970.
    pub fn invoice_expires_at(&self) -> u64 {
        self.invoice_expires_at
    }

    /// The receive address the claim pays.
    pub fn destination(&self) -> &Address {
        &self.destination
    }

    /// How far the swap has got.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The tree and the output, rebuilt from what the state holds.
    pub fn tree(&self) -> Result<SwapTree, Refused> {
        SwapTree::build(
            &self.secrets.claim_public(),
            &self.refund_key,
            &self.secrets.preimage_hash(),
            self.timeout,
            self.order,
        )
    }

    /// The lockup address, rebuilt.
    pub fn lockup_address(&self) -> Result<Address, Refused> {
        Ok(self.tree()?.address(self.network))
    }

    /// SW-53: the state after the claim was sent. A claimed swap is refused.
    pub fn claimed(&self, txid: Txid) -> Result<Self, Refused> {
        if self.phase != Phase::Waiting {
            return Err(Refused::AlreadyClaimed);
        }
        let mut next = self.clone();
        next.phase = Phase::Claimed(txid);
        Ok(next)
    }

    /// SW-46: the state after the claim was built again at another fee
    /// rate: the same phase, with the transaction id of the new claim. A
    /// swap that was not claimed is refused.
    pub fn reclaimed(&self, txid: Txid) -> Result<Self, Refused> {
        if !matches!(self.phase, Phase::Claimed(_)) {
            return Err(Refused::NotReady("no claim has been sent"));
        }
        let mut next = self.clone();
        next.phase = Phase::Claimed(txid);
        Ok(next)
    }

    /// SW-50: the file.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256 + self.invoice.len());
        out.extend_from_slice(&MAGIC_SWAP_STATE);
        out.push(STATE_VERSION);
        out.push(network_byte(self.network).unwrap_or(u8::MAX));
        out.push(self.order.to_byte());
        out.push(u8::from(self.phase != Phase::Waiting));
        out.extend_from_slice(self.secrets.preimage());
        out.extend_from_slice(&self.secrets.claim_key().secret_bytes());
        out.extend_from_slice(&self.refund_key.serialize());
        out.extend_from_slice(&self.timeout.to_be_bytes());
        out.extend_from_slice(&self.onchain_sats.to_be_bytes());
        out.extend_from_slice(&self.invoice_sats.to_be_bytes());
        out.extend_from_slice(&self.invoice_expires_at.to_be_bytes());
        out.push(u8::try_from(self.id.len()).unwrap_or(u8::MAX));
        out.extend_from_slice(self.id.as_bytes());
        out.extend_from_slice(
            &u16::try_from(self.invoice.len())
                .unwrap_or(u16::MAX)
                .to_be_bytes(),
        );
        out.extend_from_slice(self.invoice.as_bytes());
        let dest = self.destination.to_string();
        out.push(u8::try_from(dest.len()).unwrap_or(u8::MAX));
        out.extend_from_slice(dest.as_bytes());
        if let Phase::Claimed(txid) = self.phase {
            out.extend_from_slice(txid.as_ref());
        }
        out
    }

    /// SW-50 to SW-52: read a file, or refuse it.
    pub fn decode(bytes: &[u8]) -> Result<Self, Refused> {
        let mut c = Cursor(bytes);
        if c.array::<4>()? != MAGIC_SWAP_STATE {
            return Err(Refused::BadState("magic"));
        }
        if c.byte()? != STATE_VERSION {
            return Err(Refused::BadState("version"));
        }
        let network = network_from(c.byte()?).ok_or(Refused::BadState("network"))?;
        let order = KeyOrder::from_byte(c.byte()?).ok_or(Refused::BadState("key order"))?;
        let claimed = match c.byte()? {
            0 => false,
            1 => true,
            _ => return Err(Refused::BadState("phase")),
        };
        let secrets = Secrets::from_random(c.array()?, c.array()?)?;
        let refund_key = PublicKey::from_slice(&c.array::<33>()?)
            .map_err(|_| Refused::BadState("refund key"))?;
        let timeout = u32::from_be_bytes(c.array()?);
        let onchain_sats = u64::from_be_bytes(c.array()?);
        let invoice_sats = u64::from_be_bytes(c.array()?);
        let invoice_expires_at = u64::from_be_bytes(c.array()?);
        let id_len = usize::from(c.byte()?);
        let id = c.text(id_len)?;
        let invoice_len = usize::from(u16::from_be_bytes(c.array()?));
        if invoice_len > MAX_INVOICE_BYTES {
            return Err(Refused::BadState("invoice"));
        }
        let invoice = c.text(invoice_len)?;
        let dest_len = usize::from(c.byte()?);
        let dest = c.text(dest_len)?;
        let destination = Address::from_str(&dest)
            .ok()
            .and_then(|a| a.require_network(network).ok())
            .ok_or(Refused::BadState("receive address"))?;
        let phase = if claimed {
            Phase::Claimed(Txid::from_byte_array(c.array()?))
        } else {
            Phase::Waiting
        };
        if !c.0.is_empty() {
            return Err(Refused::BadState("too long"));
        }
        let state = Self {
            id,
            network,
            secrets,
            refund_key,
            order,
            timeout,
            onchain_sats,
            invoice_sats,
            invoice_expires_at,
            invoice,
            destination,
            phase,
        };
        state.validate()?;
        // The tree must rebuild.
        state.tree()?;
        if state.encode() != bytes {
            return Err(Refused::BadState("not canonical"));
        }
        Ok(state)
    }
}
