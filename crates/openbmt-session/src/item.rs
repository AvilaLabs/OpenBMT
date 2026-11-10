//! The items of a request (AS-06 to AS-11).

use openbmt_core::batch::{AddressBatch, SignedBatch, MAX_COUNT};
use openbmt_core::identity::{DeviceId, Sig};
use openbmt_core::pairing::{name_is_valid, PairRequest};
use openbmt_core::settings::{SignedSettings, MAX_DEVICES};
use openbmt_core::wire::{DecodeError, Reader};

use crate::error::EncodeError;
use crate::text::{put_label_text, put_len, read_label_text};
use crate::{MAX_OUTPUT_LABELS, MAX_SESSION_BYTES};

/// The kind of an item, which is also its byte on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// A transaction to sign (AS-06).
    Spend = 1,
    /// An address batch to co-sign (AS-08).
    BatchRefill = 2,
    /// Settings to co-sign (AS-09).
    Settings = 3,
    /// A pairing to sign (AS-10).
    Pairing = 4,
    /// A health check (AS-11).
    HealthCheck = 5,
}

impl ItemKind {
    /// The kind a byte stands for.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Spend),
            2 => Some(Self::BatchRefill),
            3 => Some(Self::Settings),
            4 => Some(Self::Pairing),
            5 => Some(Self::HealthCheck),
            _ => None,
        }
    }

    /// The kind's byte.
    pub fn byte(self) -> u8 {
        self as u8
    }
}

/// A value in the shop's currency, as the owner phone states it (AS-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShopValue {
    /// Minor units (cents, for dollars).
    pub minor: u64,
    /// Three upper-case ASCII letters.
    pub currency: [u8; 3],
}

impl ShopValue {
    fn is_valid(&self) -> bool {
        self.currency.iter().all(u8::is_ascii_uppercase)
    }
}

/// What the owner phone says an output is for. It proves nothing (AS-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// A refund of a sale.
    Refund {
        /// The counter that made the sale.
        counter: DeviceId,
        /// The sale's receive index on that counter.
        sale_index: u32,
        /// The cashier's name.
        cashier: String,
        /// The refund's value in the shop's currency, if the owner phone
        /// states one.
        shop_value: Option<ShopValue>,
    },
    /// A cash-out to a destination with this label.
    CashOut {
        /// The destination's label.
        destination: String,
    },
    /// Change back to the wallet.
    Change,
}

/// A label and the output it names (AS-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLabel {
    /// The output's position in the transaction.
    pub output: u32,
    /// The label.
    pub label: Label,
}

/// An item of a request (AS-06 to AS-11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A transaction for the signer to sign (AS-06).
    Spend {
        /// The PSBT, as bytes. This crate does not read it.
        psbt: Vec<u8>,
        /// The owner phone's labels, in ascending output order.
        labels: Vec<OutputLabel>,
    },
    /// An address batch with the owner's signature (AS-08).
    BatchRefill {
        /// The batch.
        batch: AddressBatch,
        /// The owner's signature over the batch hash.
        owner_sig: Sig,
    },
    /// Settings with the owner's signature and no Key app signature (AS-09).
    Settings {
        /// The settings. Their `approver_sig` must be `None`.
        signed: SignedSettings,
    },
    /// A counter's pairing request (AS-10).
    Pairing {
        /// The request the counter made.
        request: PairRequest,
        /// The settings version the owner phone says is in force.
        settings_version: u32,
    },
    /// A health check (AS-11).
    HealthCheck {
        /// The number the Key app signs.
        nonce: [u8; 32],
    },
}

impl Item {
    /// The item's kind.
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Spend { .. } => ItemKind::Spend,
            Self::BatchRefill { .. } => ItemKind::BatchRefill,
            Self::Settings { .. } => ItemKind::Settings,
            Self::Pairing { .. } => ItemKind::Pairing,
            Self::HealthCheck { .. } => ItemKind::HealthCheck,
        }
    }
}

/// An item and its number (AS-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestItem {
    /// The item's number. Numbers strictly ascend within a request.
    pub number: u32,
    /// The item.
    pub item: Item,
}

// ---- encoding ------------------------------------------------------------

fn put_label(out: &mut Vec<u8>, l: &OutputLabel) -> Result<(), EncodeError> {
    out.extend_from_slice(&l.output.to_le_bytes());
    match &l.label {
        Label::Refund {
            counter,
            sale_index,
            cashier,
            shop_value,
        } => {
            out.push(1);
            out.extend_from_slice(&counter.to_bytes());
            out.extend_from_slice(&sale_index.to_le_bytes());
            put_label_text(out, cashier)?;
            match shop_value {
                Some(v) => {
                    if !v.is_valid() {
                        return Err(EncodeError::BadText("currency code"));
                    }
                    out.push(1);
                    out.extend_from_slice(&v.minor.to_le_bytes());
                    out.extend_from_slice(&v.currency);
                }
                None => out.push(0),
            }
        }
        Label::CashOut { destination } => {
            out.push(2);
            put_label_text(out, destination)?;
        }
        Label::Change => out.push(3),
    }
    Ok(())
}

/// The bytes of an item's body.
pub(crate) fn encode_body(item: &Item) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    match item {
        Item::Spend { psbt, labels } => {
            if psbt.len() > MAX_SESSION_BYTES {
                return Err(EncodeError::TooLarge);
            }
            if labels.len() > MAX_OUTPUT_LABELS {
                return Err(EncodeError::BadItem("more than 100 labels"));
            }
            if labels.windows(2).any(|w| match w {
                [a, b] => a.output >= b.output,
                _ => false,
            }) {
                return Err(EncodeError::BadItem("labels are not in ascending order"));
            }
            put_len(&mut out, psbt.len())?;
            out.extend_from_slice(psbt);
            put_len(&mut out, labels.len())?;
            for l in labels {
                put_label(&mut out, l)?;
            }
        }
        Item::BatchRefill { batch, owner_sig } => {
            // `SignedBatch::to_bytes` panics on a script over 65,535 bytes,
            // so the shape it relies on is checked first.
            if batch.scripts.len() != batch.count as usize
                || batch.count > MAX_COUNT
                || batch.scripts.iter().any(|s| s.len() != 34)
            {
                return Err(EncodeError::BadItem("batch is not in encodable shape"));
            }
            let signed = SignedBatch {
                batch: batch.clone(),
                owner_sig: *owner_sig,
                approver_sig: Sig([0; 64]),
            };
            out.extend_from_slice(&signed.to_bytes());
        }
        Item::Settings { signed } => {
            if signed.approver_sig.is_some() {
                return Err(EncodeError::BadItem(
                    "settings already hold a Key app signature",
                ));
            }
            if signed.settings.devices.len() > MAX_DEVICES {
                return Err(EncodeError::BadItem("settings list too many devices"));
            }
            out.extend_from_slice(&signed.to_bytes());
        }
        Item::Pairing {
            request,
            settings_version,
        } => {
            if !name_is_valid(&request.name) {
                return Err(EncodeError::BadItem("counter name is not allowed"));
            }
            out.extend_from_slice(&settings_version.to_le_bytes());
            out.extend_from_slice(&request.to_bytes());
        }
        Item::HealthCheck { nonce } => out.extend_from_slice(nonce),
    }
    if out.len() > MAX_SESSION_BYTES {
        return Err(EncodeError::TooLarge);
    }
    Ok(out)
}

// ---- decoding ------------------------------------------------------------

fn read_label(r: &mut Reader<'_>) -> Result<OutputLabel, DecodeError> {
    let output = r.u32_le()?;
    let label = match r.u8()? {
        1 => {
            let counter = r.device_id()?;
            let sale_index = r.u32_le()?;
            let cashier = read_label_text(r)?;
            let shop_value = if r.flag()? {
                let minor = r.u64_le()?;
                let currency = r.array::<3>()?;
                let v = ShopValue { minor, currency };
                if !v.is_valid() {
                    return Err(DecodeError::NonCanonical("currency code is not letters"));
                }
                Some(v)
            } else {
                None
            };
            Label::Refund {
                counter,
                sale_index,
                cashier,
                shop_value,
            }
        }
        2 => Label::CashOut {
            destination: read_label_text(r)?,
        },
        3 => Label::Change,
        _ => return Err(DecodeError::NonCanonical("unknown label kind")),
    };
    Ok(OutputLabel { output, label })
}

/// Read an item of `kind` from exactly `body`.
pub(crate) fn decode_body(kind: ItemKind, body: &[u8]) -> Result<Item, DecodeError> {
    match kind {
        ItemKind::Spend => {
            let mut r = Reader::new(body);
            let psbt_len = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
            let psbt = r.take(psbt_len)?.to_vec();
            let n = r.count(MAX_OUTPUT_LABELS, 5)?;
            let mut labels: Vec<OutputLabel> = Vec::with_capacity(n);
            for _ in 0..n {
                let l = read_label(&mut r)?;
                if labels.last().is_some_and(|p| p.output >= l.output) {
                    return Err(DecodeError::NonCanonical(
                        "labels are not in ascending order",
                    ));
                }
                labels.push(l);
            }
            r.finish()?;
            Ok(Item::Spend { psbt, labels })
        }
        ItemKind::BatchRefill => {
            let signed = SignedBatch::from_bytes(body)?;
            if signed.approver_sig.0 != [0; 64] {
                return Err(DecodeError::NonCanonical(
                    "batch carries a Key app signature",
                ));
            }
            Ok(Item::BatchRefill {
                batch: signed.batch,
                owner_sig: signed.owner_sig,
            })
        }
        ItemKind::Settings => {
            let signed = SignedSettings::from_bytes(body)?;
            if signed.approver_sig.is_some() {
                return Err(DecodeError::NonCanonical(
                    "settings carry a Key app signature",
                ));
            }
            Ok(Item::Settings { signed })
        }
        ItemKind::Pairing => {
            let mut r = Reader::new(body);
            let settings_version = r.u32_le()?;
            let rest = r.take(body.len().saturating_sub(4))?;
            r.finish()?;
            let request = PairRequest::from_bytes(rest)?;
            Ok(Item::Pairing {
                request,
                settings_version,
            })
        }
        ItemKind::HealthCheck => {
            let mut r = Reader::new(body);
            let nonce = r.array::<32>()?;
            r.finish()?;
            Ok(Item::HealthCheck { nonce })
        }
    }
}
