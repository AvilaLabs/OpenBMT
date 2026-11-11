//! The swap tree and the output it makes (SW-25, SW-26).

use bitcoin::hashes::{ripemd160, Hash};
use bitcoin::opcodes::all::{
    OP_CHECKSIG, OP_CHECKSIGVERIFY, OP_CLTV, OP_EQUALVERIFY, OP_HASH160, OP_SIZE,
};
use bitcoin::script::Builder;
use bitcoin::secp256k1::{PublicKey, Secp256k1};
use bitcoin::taproot::{ControlBlock, LeafVersion, TaprootBuilder, TaprootSpendInfo};
use bitcoin::{Address, Network, ScriptBuf, XOnlyPublicKey};

use crate::error::Refused;
use crate::keys::key_agg;

/// The order the two keys went into the aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOrder {
    /// The service's refund key first, then the shop's claim key.
    RefundFirst,
    /// The shop's claim key first, then the service's refund key.
    ClaimFirst,
}

impl KeyOrder {
    /// The byte the state file keeps.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::RefundFirst => 0,
            Self::ClaimFirst => 1,
        }
    }

    /// The order a byte of the state file means.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::RefundFirst),
            1 => Some(Self::ClaimFirst),
            _ => None,
        }
    }
}

/// SW-25: the claim leaf, spendable only with the preimage of the hash and a
/// signature of the claim key.
pub fn claim_script(claim: &PublicKey, preimage_hash: &[u8; 32]) -> ScriptBuf {
    let digest = ripemd160::Hash::hash(preimage_hash).to_byte_array();
    Builder::new()
        .push_opcode(OP_SIZE)
        .push_int(32)
        .push_opcode(OP_EQUALVERIFY)
        .push_opcode(OP_HASH160)
        .push_slice(digest)
        .push_opcode(OP_EQUALVERIFY)
        .push_x_only_key(&claim.x_only_public_key().0)
        .push_opcode(OP_CHECKSIG)
        .into_script()
}

/// SW-25: the refund leaf, for the service after the timeout.
pub fn refund_script(refund: &PublicKey, timeout: u32) -> ScriptBuf {
    Builder::new()
        .push_x_only_key(&refund.x_only_public_key().0)
        .push_opcode(OP_CHECKSIGVERIFY)
        .push_int(i64::from(timeout))
        .push_opcode(OP_CLTV)
        .into_script()
}

/// The tree and the output of a swap.
#[derive(Debug, Clone)]
pub struct SwapTree {
    claim: ScriptBuf,
    refund: ScriptBuf,
    internal: XOnlyPublicKey,
    info: TaprootSpendInfo,
}

impl SwapTree {
    /// Rebuild the tree and the output key from the keys, the preimage hash
    /// and the timeout.
    pub fn build(
        claim_key: &PublicKey,
        refund_key: &PublicKey,
        preimage_hash: &[u8; 32],
        timeout: u32,
        order: KeyOrder,
    ) -> Result<Self, Refused> {
        let keys = match order {
            KeyOrder::RefundFirst => [*refund_key, *claim_key],
            KeyOrder::ClaimFirst => [*claim_key, *refund_key],
        };
        let internal = key_agg(&keys)?;
        let claim = claim_script(claim_key, preimage_hash);
        let refund = refund_script(refund_key, timeout);
        let info = TaprootBuilder::new()
            .add_leaf(1, claim.clone())
            .and_then(|b| b.add_leaf(1, refund.clone()))
            .map_err(|_| Refused::LeafMismatch("claim"))?
            .finalize(&Secp256k1::verification_only(), internal)
            .map_err(|_| Refused::LeafMismatch("claim"))?;
        Ok(Self {
            claim,
            refund,
            internal,
            info,
        })
    }

    /// The claim leaf.
    pub fn claim_leaf(&self) -> &ScriptBuf {
        &self.claim
    }

    /// The refund leaf.
    pub fn refund_leaf(&self) -> &ScriptBuf {
        &self.refund
    }

    /// The key of the output before the tweak.
    pub fn internal_key(&self) -> XOnlyPublicKey {
        self.internal
    }

    /// The script of the lockup output.
    pub fn script_pubkey(&self) -> ScriptBuf {
        ScriptBuf::new_p2tr_tweaked(self.info.output_key())
    }

    /// The address of the lockup output.
    pub fn address(&self, network: Network) -> Address {
        Address::p2tr_tweaked(self.info.output_key(), network)
    }

    /// The control block that spends the claim leaf.
    pub fn claim_control_block(&self) -> Option<ControlBlock> {
        self.info
            .control_block(&(self.claim.clone(), LeafVersion::TapScript))
    }
}
