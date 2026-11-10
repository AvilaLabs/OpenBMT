//! The signer state's byte form (`OBMK`, version 1).
//!
//! After the magic and version, in this order, all integers little-endian:
//!
//! ```text
//! network u8 (0 bitcoin, 1 testnet, 2 signet, 3 regtest)
//! own slot u8 (1..=3)
//! key 3 flag u8, then its slot u8 only if the flag is 1
//! wallet canonical string: u16 length, bytes
//! receive mark u32 | change mark u32 | latest time seen u64
//! ceilings for other destinations: per tx, per day, per week (3 x u64)
//! ceilings for allowlisted destinations (3 x u64)
//! fee ceilings: sat per vB, absolute sats (2 x u64)
//! allowlist: u32 count, then per entry, in strictly ascending script bytes:
//!     script u8 length + bytes | label u8 byte length + UTF-8 | enrolled_at u64
//! spend log: u32 count, then per entry, in strictly ascending txid bytes:
//!     txid 32 | time u64 | other u64 | allowlisted u64 | fee u64
//!     | u32 input count | inputs (txid 32 + vout u32) in strictly ascending order
//! ```
//!
//! Reading is strict (see [`openbmt_core::wire`]) and checks again every
//! invariant that the live state keeps. The trust anchors (the network and
//! this device's account key) are not read from the bytes: the caller
//! supplies them, and a snapshot that does not match them is refused.

use bitcoin::bip32::Xpub;
use bitcoin::hashes::Hash;
use bitcoin::{Network, OutPoint, ScriptBuf, Txid};
use openbmt_core::wallet::WalletDescription;
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::state::{
    is_payment_script, label_is_valid, outpoint_key, AllowlistEntry, Ceilings, FeeCeilings,
    LogEntry, LogMap, SignerState,
};
use crate::{
    MAX_ALLOWLIST, MAX_FEE_RATE_SAT_PER_VB, MAX_FEE_SATS, MAX_INPUTS, MAX_LOG_ENTRIES,
    MAX_LOG_INPUTS, MAX_MONEY_SATS,
};

/// The byte that stands for a network, for the four the signer supports.
pub(crate) fn network_byte(network: Network) -> Option<u8> {
    match network {
        Network::Bitcoin => Some(0),
        Network::Testnet => Some(1),
        Network::Signet => Some(2),
        Network::Regtest => Some(3),
        _ => None,
    }
}

fn bad(why: &'static str) -> DecodeError {
    DecodeError::NonCanonical(why)
}

fn read_ceilings(r: &mut Reader<'_>) -> Result<Ceilings, DecodeError> {
    let c = Ceilings {
        per_tx: r.u64_le()?,
        per_day: r.u64_le()?,
        per_week: r.u64_le()?,
    };
    if c.per_tx > MAX_MONEY_SATS || c.per_day > MAX_MONEY_SATS || c.per_week > MAX_MONEY_SATS {
        return Err(bad("ceiling is above all bitcoin there can be"));
    }
    Ok(c)
}

fn put_ceilings(out: &mut Vec<u8>, c: &Ceilings) {
    for v in [c.per_tx, c.per_day, c.per_week] {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

impl SignerState {
    /// The state's one byte form.
    pub fn to_bytes(&self) -> Vec<u8> {
        let text = self.wallet.canonical_string().as_bytes();
        let mut out = wire::start(&wire::MAGIC_SIGNER_STATE, 256 + text.len());
        out.push(network_byte(self.wallet.network()).expect("a supported network is registered"));
        out.push(self.own_slot);
        match self.key3_slot {
            Some(s) => out.extend_from_slice(&[1, s]),
            None => out.push(0),
        }
        let len = u16::try_from(text.len()).expect("a wallet description is far below 64 KiB");
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(text);
        out.extend_from_slice(&self.receive_mark.to_le_bytes());
        out.extend_from_slice(&self.change_mark.to_le_bytes());
        out.extend_from_slice(&self.latest_time_seen.to_le_bytes());
        put_ceilings(&mut out, &self.other);
        put_ceilings(&mut out, &self.allowlisted);
        out.extend_from_slice(&self.fee.sat_per_vb.to_le_bytes());
        out.extend_from_slice(&self.fee.absolute_sats.to_le_bytes());
        out.extend_from_slice(&wire::len_u32(self.allowlist.len()).to_le_bytes());
        for e in &self.allowlist {
            out.push(u8::try_from(e.script.len()).expect("a payment script is short"));
            out.extend_from_slice(e.script.as_bytes());
            out.push(u8::try_from(e.label.len()).expect("a label is at most 255 bytes"));
            out.extend_from_slice(e.label.as_bytes());
            out.extend_from_slice(&e.enrolled_at.to_le_bytes());
        }
        out.extend_from_slice(&wire::len_u32(self.log.len()).to_le_bytes());
        for e in self.log.values() {
            out.extend_from_slice(&e.txid.to_byte_array());
            for v in [e.time, e.other_sats, e.allowlisted_sats, e.fee_sats] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.extend_from_slice(&wire::len_u32(e.inputs.len()).to_le_bytes());
            for i in &e.inputs {
                out.extend_from_slice(&i.txid.to_byte_array());
                out.extend_from_slice(&i.vout.to_le_bytes());
            }
        }
        out
    }

    /// Read a state back, strictly.
    ///
    /// The trust anchors are supplied again and never read from the bytes:
    /// the snapshot must name `network`, and its own slot must hold
    /// `own_account_key` in full, or it is refused. Every invariant of the
    /// live state is checked again.
    pub fn from_bytes(
        bytes: &[u8],
        network: Network,
        own_account_key: &Xpub,
    ) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_SIGNER_STATE)?;
        let want_network = network_byte(network).ok_or(bad("network is not supported"))?;
        if r.u8()? != want_network {
            return Err(bad("snapshot is for another network"));
        }
        let own_slot = r.u8()?;
        if !(1..=3).contains(&own_slot) {
            return Err(bad("own slot is not 1, 2 or 3"));
        }
        let key3_slot = if r.flag()? { Some(r.u8()?) } else { None };
        if let Some(s) = key3_slot {
            if !(1..=3).contains(&s) || s == own_slot {
                return Err(bad("key 3 slot is not another slot of 1, 2 or 3"));
            }
        }
        let text_len = usize::from(r.u16_le()?);
        let text = std::str::from_utf8(r.take(text_len)?)
            .map_err(|_| bad("wallet description is not UTF-8"))?;
        let wallet = WalletDescription::parse(text, network)
            .map_err(|_| bad("wallet description is not valid"))?;
        if wallet.canonical_string() != text {
            return Err(bad("wallet description is not in the pinned form"));
        }
        if !wallet.slot_matches(own_slot, own_account_key) {
            return Err(bad("snapshot is for another key"));
        }
        let receive_mark = r.u32_le()?;
        let change_mark = r.u32_le()?;
        if receive_mark > 1 << 31 || change_mark > 1 << 31 {
            return Err(bad("mark is above 2^31"));
        }
        let latest_time_seen = r.u64_le()?;
        let other = read_ceilings(&mut r)?;
        let allowlisted = read_ceilings(&mut r)?;
        let fee = FeeCeilings {
            sat_per_vb: r.u64_le()?,
            absolute_sats: r.u64_le()?,
        };
        if !(1..=MAX_FEE_RATE_SAT_PER_VB).contains(&fee.sat_per_vb)
            || !(1..=MAX_FEE_SATS).contains(&fee.absolute_sats)
        {
            return Err(bad("fee ceiling is outside its fixed bounds"));
        }

        let n = r.count(MAX_ALLOWLIST, 1 + 1 + 8)?;
        let mut allowlist: Vec<AllowlistEntry> = Vec::with_capacity(n);
        for _ in 0..n {
            let script_len = usize::from(r.u8()?);
            let script = ScriptBuf::from_bytes(r.take(script_len)?.to_vec());
            if let Some(prev) = allowlist.last() {
                if prev.script.as_bytes() >= script.as_bytes() {
                    return Err(bad("allowlist is not in ascending script order"));
                }
            }
            if !is_payment_script(&script) {
                return Err(bad("allowlist script is not a payment script"));
            }
            let label_len = usize::from(r.u8()?);
            let label = std::str::from_utf8(r.take(label_len)?)
                .map_err(|_| bad("label is not UTF-8"))?
                .to_string();
            if !label_is_valid(&label) {
                return Err(bad("label is not valid"));
            }
            let enrolled_at = r.u64_le()?;
            if enrolled_at > latest_time_seen {
                return Err(bad("destination is enrolled after the latest time seen"));
            }
            allowlist.push(AllowlistEntry {
                script,
                label,
                enrolled_at,
            });
        }

        let n = r.count(MAX_LOG_ENTRIES, 32 + 8 * 4 + 4 + 36)?;
        let mut log = LogMap::new();
        let mut last_txid: Option<[u8; 32]> = None;
        let mut total_inputs = 0usize;
        let mut other_sum = 0u128;
        let mut allowlisted_sum = 0u128;
        for _ in 0..n {
            let txid_bytes = r.array::<32>()?;
            if last_txid.is_some_and(|prev| prev >= txid_bytes) {
                return Err(bad("spend log is not in ascending transaction order"));
            }
            last_txid = Some(txid_bytes);
            let time = r.u64_le()?;
            let other_sats = r.u64_le()?;
            let allowlisted_sats = r.u64_le()?;
            let fee_sats = r.u64_le()?;
            if time > latest_time_seen {
                return Err(bad("spend log entry is dated after the latest time seen"));
            }
            if other_sats > MAX_MONEY_SATS
                || allowlisted_sats > MAX_MONEY_SATS
                || fee_sats > MAX_MONEY_SATS
            {
                return Err(bad("spend log amount is above all bitcoin there can be"));
            }
            other_sum += u128::from(other_sats) + u128::from(fee_sats);
            allowlisted_sum += u128::from(allowlisted_sats);
            let k = r.count(MAX_INPUTS, 36)?;
            if k == 0 {
                return Err(bad("spend log entry has no inputs"));
            }
            total_inputs += k;
            if total_inputs > MAX_LOG_INPUTS {
                return Err(DecodeError::TooLarge);
            }
            let mut inputs: Vec<OutPoint> = Vec::with_capacity(k);
            for _ in 0..k {
                let op = OutPoint {
                    txid: Txid::from_byte_array(r.array::<32>()?),
                    vout: r.u32_le()?,
                };
                if inputs
                    .last()
                    .is_some_and(|prev| outpoint_key(prev) >= outpoint_key(&op))
                {
                    return Err(bad("spend log inputs are not in ascending order"));
                }
                inputs.push(op);
            }
            log.insert(
                txid_bytes,
                LogEntry {
                    txid: Txid::from_byte_array(txid_bytes),
                    time,
                    other_sats,
                    allowlisted_sats,
                    fee_sats,
                    inputs,
                },
            );
        }
        if u64::try_from(other_sum).is_err() || u64::try_from(allowlisted_sum).is_err() {
            return Err(bad("spend log totals overflow"));
        }
        r.finish()?;
        Ok(Self {
            wallet,
            own_slot,
            key3_slot,
            receive_mark,
            change_mark,
            latest_time_seen,
            other,
            allowlisted,
            fee,
            allowlist,
            log,
        })
    }
}
