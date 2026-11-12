//! The status of a swap, read from `GET /v2/swap/{id}` (SW-70).
//!
//! The status tells the shop when to look; it is never believed about money.

use bitcoin::consensus::deserialize;
use bitcoin::hex::FromHex;
use bitcoin::{Transaction, Txid};
use openbmt_json::{Limits, Value};
use std::str::FromStr;

use crate::error::Refused;
use crate::{MAX_HTTP_BYTES, MAX_REPLY_DEPTH};

/// The longest status name that is kept when it is not a known one.
const MAX_STATUS_BYTES: usize = 64;

/// What the service says the swap is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusKind {
    /// `swap.created`: the invoice exists and is not paid.
    SwapCreated,
    /// `transaction.mempool`: the service's lockup is in the mempool.
    TransactionMempool,
    /// `transaction.confirmed`: the service's lockup is in a block.
    TransactionConfirmed,
    /// `invoice.settled`: the service has the preimage.
    InvoiceSettled,
    /// `invoice.expired`: the invoice ran out unpaid.
    InvoiceExpired,
    /// `transaction.failed`: the service could not make the lockup.
    TransactionFailed,
    /// `swap.expired`: the swap ran out.
    SwapExpired,
    /// `transaction.refunded`: the service took its lockup back.
    TransactionRefunded,
    /// A status this library does not know, as the service wrote it. It is
    /// not guessed at.
    Unknown(String),
}

/// A status reply, read and not yet believed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    kind: StatusKind,
    txid: Option<Txid>,
    tx: Option<Transaction>,
}

impl Status {
    /// SW-70: read the body of the reply to `GET /v2/swap/{id}`. At
    /// `transaction.mempool` and `transaction.confirmed`, the transaction's
    /// `hex` and `id` are read if they are given, and the id must be the
    /// transaction's.
    pub fn parse(body: &[u8]) -> Result<Self, Refused> {
        let limits = Limits {
            max_bytes: MAX_HTTP_BYTES,
            max_depth: MAX_REPLY_DEPTH,
            ..Limits::DEFAULT
        };
        let doc = openbmt_json::parse(body, &limits).map_err(|_| Refused::BadStatus("document"))?;
        let name = doc
            .get("status")
            .and_then(Value::as_str)
            .ok_or(Refused::BadStatus("status"))?;
        let kind = match name {
            "swap.created" => StatusKind::SwapCreated,
            "transaction.mempool" => StatusKind::TransactionMempool,
            "transaction.confirmed" => StatusKind::TransactionConfirmed,
            "invoice.settled" => StatusKind::InvoiceSettled,
            "invoice.expired" => StatusKind::InvoiceExpired,
            "transaction.failed" => StatusKind::TransactionFailed,
            "swap.expired" => StatusKind::SwapExpired,
            "transaction.refunded" => StatusKind::TransactionRefunded,
            other => {
                if other.is_empty()
                    || other.len() > MAX_STATUS_BYTES
                    || !other.bytes().all(|b| b.is_ascii_graphic())
                {
                    return Err(Refused::BadStatus("status"));
                }
                StatusKind::Unknown(other.to_string())
            }
        };
        let (mut txid, mut tx) = (None, None);
        if matches!(
            kind,
            StatusKind::TransactionMempool | StatusKind::TransactionConfirmed
        ) {
            if let Some(t) = doc.get("transaction") {
                if t.as_object().is_none() {
                    return Err(Refused::BadStatus("transaction"));
                }
                if let Some(h) = t.get("hex") {
                    let bytes = h
                        .as_str()
                        .and_then(|s| Vec::<u8>::from_hex(s).ok())
                        .ok_or(Refused::BadStatus("transaction.hex"))?;
                    tx = Some(
                        deserialize::<Transaction>(&bytes)
                            .map_err(|_| Refused::BadStatus("transaction.hex"))?,
                    );
                }
                if let Some(i) = t.get("id") {
                    txid = Some(
                        i.as_str()
                            .filter(|s| s.len() == 64)
                            .and_then(|s| Txid::from_str(s).ok())
                            .ok_or(Refused::BadStatus("transaction.id"))?,
                    );
                }
                if let (Some(id), Some(t)) = (txid, &tx) {
                    if t.compute_txid() != id {
                        return Err(Refused::BadStatus("transaction.id"));
                    }
                }
                if txid.is_none() {
                    txid = tx.as_ref().map(Transaction::compute_txid);
                }
            }
        }
        Ok(Self { kind, txid, tx })
    }

    /// What the service says the swap is doing.
    pub fn kind(&self) -> &StatusKind {
        &self.kind
    }

    /// The id of the lockup transaction, if the status gave one.
    pub fn txid(&self) -> Option<Txid> {
        self.txid
    }

    /// The lockup transaction, if the status gave its bytes. Its bytes are
    /// checked by the shop (SW-30); the service's word is not.
    pub fn transaction(&self) -> Option<&Transaction> {
        self.tx.as_ref()
    }
}
