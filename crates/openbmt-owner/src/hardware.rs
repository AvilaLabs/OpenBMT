//! A hardware signer as key 2 (AS-90 to AS-99): the spends that go to it
//! and not to the Key app, the file it is given, what it returns, and the
//! registration of the wallet on it.

use std::convert::Infallible;
use std::fmt;

use bitcoin::bip32::{ChildNumber, Xpriv};
use bitcoin::consensus::deserialize;
use bitcoin::psbt::Psbt;
use bitcoin::{Address, Network, Transaction, Txid};
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::wallet::{coin_type, WalletDescription};
use openbmt_session::{Item, Request, RequestItem};
use openbmt_signer::{SignError, SignerState};
use openbmt_spend::{
    check_finished, finalize, psbt_for_signer, take_signer_signatures, transaction_of_final_file,
};
use openbmt_ur::{wsh_sortedmulti_cbor, OutputKey};

use crate::codec;
use crate::error::{OwnerError, Refusal};
use crate::say::stop;
use crate::state::{Frozen, Owner};
use crate::types::{Deliverable, Delivery, Stage};
use crate::{MAX_OUTBOX, MAX_SNAPSHOT_BYTES};

/// AS-99: the magic of the record of the first address check.
pub const MAGIC_FIRST_ADDRESS: [u8; 4] = *b"OBM0";

/// AS-93: the most bytes of what a signer returns that are read.
pub const MAX_RETURNED_BYTES: usize = 4 << 20;

/// AS-98: the most characters shown as one plain-text QR code; a longer text
/// goes as the animated codes or a file.
pub const PLAIN_QR_MAX_CHARS: usize = 1500;

/// AS-97: the most characters of the name a signer is given for the wallet.
pub const MAX_WALLET_NAME_CHARS: usize = 15;

/// The slot of the hardware signer's key.
const SIGNER_SLOT: u8 = 2;

/// Why a step with a hardware signer was refused. Nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareRefusal {
    /// AS-90: the session or the item is not a spend meant for a hardware
    /// signer.
    NotASignerSpend,
    /// AS-91: the transaction is not the one that was frozen, or key 1's
    /// signer refused it. The text says which.
    Key1(String),
    /// AS-92: nothing was given to the signer for this item yet.
    NotExported,
    /// AS-93: what came back is not a PSBT or a transaction in a form that
    /// is read. The text says what is wrong.
    Unreadable(&'static str),
    /// AS-94: what came back holds another transaction than the one given.
    DifferentTransaction,
    /// AS-95: the signer's key is missing on an input, or a signature does
    /// not verify. The text says which.
    NotSigned(String),
    /// AS-96: the finished transaction does not pass the checks. The text
    /// says which.
    Finished(String),
    /// AS-96: too many finished objects are waiting.
    OutboxFull,
    /// AS-97: the wallet cannot be written for a signer.
    Registration(&'static str),
    /// AS-99: the record is not one this wallet's.
    Record(&'static str),
}

impl fmt::Display for HardwareRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotASignerSpend => write!(f, "AS-90: this is not a spend for a hardware signer"),
            Self::Key1(why) => write!(f, "AS-91: {why}"),
            Self::NotExported => write!(f, "AS-92: nothing was given to the signer yet"),
            Self::Unreadable(why) => write!(f, "AS-93: what came back cannot be read: {why}"),
            Self::DifferentTransaction => write!(
                f,
                "AS-94: what came back holds another transaction than the one given"
            ),
            Self::NotSigned(why) => write!(f, "AS-95: {why}"),
            Self::Finished(why) => write!(f, "AS-96: {why}"),
            Self::OutboxFull => write!(
                f,
                "AS-96: too many finished objects are waiting to be delivered"
            ),
            Self::Registration(why) => write!(f, "AS-97: {why}"),
            Self::Record(why) => write!(f, "AS-99: {why}"),
        }
    }
}

impl std::error::Error for HardwareRefusal {}

impl From<HardwareRefusal> for Refusal {
    fn from(h: HardwareRefusal) -> Self {
        Self::Hardware(h)
    }
}

impl<E> From<HardwareRefusal> for OwnerError<E> {
    fn from(h: HardwareRefusal) -> Self {
        Self::Refused(Refusal::Hardware(h))
    }
}

/// The ways a refusal of this module can come, with a value of each, for the
/// words of every state to be checked (SC-12).
pub fn all_refusals() -> Vec<(&'static str, HardwareRefusal)> {
    vec![
        ("NotASignerSpend", HardwareRefusal::NotASignerSpend),
        (
            "Key1",
            HardwareRefusal::Key1("the transaction is not the one that was frozen".to_string()),
        ),
        ("NotExported", HardwareRefusal::NotExported),
        (
            "Unreadable",
            HardwareRefusal::Unreadable("it is not a PSBT or a transaction"),
        ),
        (
            "DifferentTransaction",
            HardwareRefusal::DifferentTransaction,
        ),
        (
            "NotSigned",
            HardwareRefusal::NotSigned("input 0 has no signature of the signer".to_string()),
        ),
        (
            "Finished",
            HardwareRefusal::Finished("the witness of input 0 does not verify".to_string()),
        ),
        ("OutboxFull", HardwareRefusal::OutboxFull),
        (
            "Registration",
            HardwareRefusal::Registration("the wallet has no name for a signer"),
        ),
        (
            "Record",
            HardwareRefusal::Record("the record is for another wallet"),
        ),
    ]
}

impl HardwareRefusal {
    /// What the owner is told (SC-01 to SC-12). The signer's own screen is
    /// the second review of a spend, so the words say what to compare there.
    pub fn words(&self) -> Words {
        match self {
            Self::NotASignerSpend => stop(
                "Not for the signer",
                "This item is not a payment that goes to the hardware signer.",
                Who::Owner,
                "Nothing was changed. Pick a payment from the list of items for the signer.",
            )
            .code("AS-90"),
            Self::Key1(why) => stop(
                "This phone stopped it",
                "The check on this phone refused to sign the payment, so nothing was given to the signer.",
                Who::Owner,
                "Nothing was changed. Cancel the item or queue it again.",
            )
            .code("AS-91")
            .detail(why.clone()),
            Self::NotExported => stop(
                "Nothing sent to the signer",
                "This payment has not been signed on this phone and given to the signer yet.",
                Who::Owner,
                "Nothing was changed. Hand the payment to the signer first.",
            )
            .code("AS-92"),
            Self::Unreadable(why) => stop(
                "That is not from the signer",
                "What was read is not a signing file or a finished payment in a form this phone takes.",
                Who::Owner,
                "Nothing was changed. Scan or open what the signer made again, or read the signer's manual for its export.",
            )
            .code("AS-93")
            .detail((*why).to_string()),
            Self::DifferentTransaction => stop(
                "Another payment came back",
                "What came back is a different payment from the one given to the signer.",
                Who::Owner,
                "Nothing was changed. Do not send it. Sign the payment shown here on the signer, and bring that back.",
            )
            .code("AS-94"),
            Self::NotSigned(why) => stop(
                "This signer did not sign",
                "What came back has no valid signature of the signer's key on every input.",
                Who::Owner,
                "Nothing was changed. Check that the signer holds key 2 of this wallet, then sign again on it.",
            )
            .code("AS-95")
            .detail(why.clone()),
            Self::Finished(why) => stop(
                "The payment failed a check",
                "The finished payment is not exactly the one approved on this phone, or its signatures do not check out.",
                Who::Owner,
                "Nothing was changed and nothing was sent. Sign again on the signer and bring that back.",
            )
            .code("AS-96")
            .detail(why.clone()),
            Self::OutboxFull => stop(
                "Too many items are waiting",
                "Too many finished items wait to be sent, so this one was not taken.",
                Who::Owner,
                "Nothing was changed. Send the waiting items first, then bring this back.",
            )
            .code("AS-96"),
            Self::Registration(why) => stop(
                "Cannot write the wallet",
                "The wallet cannot be written in the form a signer takes.",
                Who::Owner,
                "Nothing was changed. Report this to the OpenBMT project with the details.",
            )
            .code("AS-97")
            .detail((*why).to_string()),
            Self::Record(why) => stop(
                "Wrong saved record",
                "The saved record of the address check is not this wallet's.",
                Who::Owner,
                "Nothing was changed. Do the address check again.",
            )
            .code("AS-99")
            .detail((*why).to_string()),
        }
    }
}

/// AS-92: the words beside the file for the signer: what its screen shows
/// and what to compare with it.
pub fn compare_words() -> Words {
    Words::new(
        Tone::Info,
        "Check it on the signer",
        "The signer shows each address, each amount and the fee; compare every one with this screen.",
        Who::Owner,
        "Approve on the signer only if they all match. The signer checks the change address against the registered wallet itself.",
    )
    .code("AS-92")
}

/// One payment of a spend, as the owner compares it with the signer's
/// screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryLine {
    /// The address.
    pub address: String,
    /// The amount in satoshis.
    pub sats: u64,
}

/// AS-92: what the signer's screen must show for a spend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerSummary {
    /// The payments, in output order.
    pub payments: Vec<SummaryLine>,
    /// The change back to the wallet, if there is any.
    pub change: Option<SummaryLine>,
    /// The fee in satoshis.
    pub fee: u64,
}

/// AS-91: the file for the signer and what to compare on its screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerFile {
    /// The PSBT, binary BIP 174 version 0, signed by key 1.
    pub psbt: Vec<u8>,
    /// The transaction's ID.
    pub txid: Txid,
    /// What the signer's screen must show.
    pub summary: SignerSummary,
    /// A name for a file: `<first 8 characters of the ID>.psbt`.
    pub file_name: String,
}

/// AS-96: what was taken from the signer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerTaken {
    /// The finished transaction's number in the outbox.
    pub id: u32,
    /// Its ID.
    pub txid: Txid,
    /// Its fee in satoshis.
    pub fee: u64,
}

/// AS-93: what came back from a signer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Returned {
    /// A PSBT, signed or not.
    Psbt(Psbt),
    /// A finished transaction.
    Transaction(Transaction),
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let b = text.as_bytes();
    if b.is_empty() || !b.len().is_multiple_of(2) {
        return None;
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let (pairs, _) = b.as_chunks::<2>();
    pairs
        .iter()
        .map(|[h, l]| Some(digit(*h)? << 4 | digit(*l)?))
        .collect()
}

/// Standard base64 with padding; no other character is allowed.
fn unbase64(text: &str) -> Option<Vec<u8>> {
    let b = text.as_bytes();
    if b.is_empty() || !b.len().is_multiple_of(4) {
        return None;
    }
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let (quads, _) = b.as_chunks::<4>();
    let mut out = Vec::with_capacity(quads.len() * 3);
    for (i, q) in quads.iter().enumerate() {
        let last = i + 1 == quads.len();
        let pad = if last {
            q.iter().rev().take_while(|c| **c == b'=').count()
        } else {
            0
        };
        if pad > 2 {
            return None;
        }
        let mut acc: u32 = 0;
        for (k, c) in q.iter().enumerate() {
            let v = if k >= 4 - pad {
                0
            } else {
                u32::from(value(*c)?)
            };
            acc = acc << 6 | v;
        }
        let bytes = acc.to_be_bytes();
        out.extend_from_slice(bytes.get(1..4 - pad)?);
    }
    Some(out)
}

/// AS-93: read what a signer returned: a binary or base64 or hexadecimal
/// PSBT, or a finished transaction in binary or hexadecimal. A PSBT whose
/// every input is final is read as the transaction it holds (SP-48). The
/// bytes may come from a file, a UR (`crypto-psbt`, `psbt`) or a BBQr of type
/// P or T, which the caller has already joined.
pub fn read_returned(bytes: &[u8]) -> Result<Returned, HardwareRefusal> {
    if bytes.is_empty() || bytes.len() > MAX_RETURNED_BYTES {
        return Err(HardwareRefusal::Unreadable("it is empty or too large"));
    }
    // A text can be read as base64 and as hexadecimal both (the digits are
    // in both alphabets): the reading that gives a PSBT, or else one that
    // gives a transaction, is the one taken.
    let mut readings: Vec<Vec<u8>> = vec![bytes.to_vec()];
    if let Ok(text) = std::str::from_utf8(bytes) {
        let text = text.trim();
        readings.extend(unbase64(text));
        readings.extend(unhex(text));
    }
    let raw = readings
        .iter()
        .find(|r| r.starts_with(b"psbt\xff"))
        .or_else(|| {
            readings
                .iter()
                .find(|r| deserialize::<Transaction>(r).is_ok())
        })
        .cloned()
        .ok_or(HardwareRefusal::Unreadable(
            "it is not a PSBT or a transaction, in binary, base64 or hexadecimal",
        ))?;
    if raw.starts_with(b"psbt\xff") {
        let psbt = Psbt::deserialize(&raw)
            .map_err(|_| HardwareRefusal::Unreadable("the PSBT is malformed"))?;
        return Ok(match transaction_of_final_file(&psbt) {
            Some(tx) => Returned::Transaction(tx),
            None => Returned::Psbt(psbt),
        });
    }
    let tx: Transaction = deserialize(&raw)
        .map_err(|_| HardwareRefusal::Unreadable("it is not a transaction either"))?;
    Ok(Returned::Transaction(tx))
}

/// The slots' names of a wallet: the printable name of at most
/// [`MAX_WALLET_NAME_CHARS`] characters a signer is given, made from the
/// shop's name: letters, digits, spaces, hyphens, underscores and dots.
pub fn wallet_name(shop: &str) -> String {
    let kept: String = shop
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        .collect();
    let name: String = kept
        .trim()
        .chars()
        .take(MAX_WALLET_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_string();
    if name.is_empty() {
        "OpenBMT".to_string()
    } else {
        name
    }
}

/// AS-97: the wallet in the forms signers take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    /// The name in the text file.
    pub name: String,
    /// The Coldcard multisig text file. Read by the Coldcard, by Jade,
    /// Passport and Keystone in their own ways. It names no path after the
    /// keys: the signer derives the receive and the change addresses itself.
    pub text_file: String,
    /// The descriptor with both chains (`/<0;1>/*`), for software that reads
    /// it, with its checksum.
    pub descriptor_both: String,
    /// The descriptor of the receive addresses (`/0/*`), with its checksum.
    /// This is the form of the UR.
    pub descriptor_receive: String,
    /// The descriptor of the change addresses (`/1/*`), with its checksum.
    pub descriptor_change: String,
    /// The CBOR of the `crypto-output` of the receive form, the message of a
    /// `ur:crypto-output`.
    pub output_cbor: Vec<u8>,
}

impl Registration {
    /// AS-98: whether the text file fits one plain-text QR code.
    pub fn text_fits_plain_qr(&self) -> bool {
        self.text_file.len() <= PLAIN_QR_MAX_CHARS
    }

    /// AS-98: whether the receive descriptor fits one plain-text QR code.
    pub fn descriptor_fits_plain_qr(&self) -> bool {
        self.descriptor_receive.len() <= PLAIN_QR_MAX_CHARS
    }
}

fn origin_steps(path: &[ChildNumber]) -> Vec<(u32, bool)> {
    path.iter()
        .map(|c| match c {
            ChildNumber::Normal { index } => (*index, false),
            ChildNumber::Hardened { index } => (*index, true),
        })
        .collect()
}

/// AS-97: the registration of `wallet` for a signer, under the name made
/// from `shop`.
pub fn registration(
    wallet: &WalletDescription,
    shop: &str,
) -> Result<Registration, HardwareRefusal> {
    let name = wallet_name(shop);
    let coin = coin_type(wallet.network());
    let mut text =
        format!("Name: {name}\nPolicy: 2 of 3\nDerivation: m/48'/{coin}'/0'/2'\nFormat: P2WSH\n\n");
    let mut keys: Vec<OutputKey> = Vec::with_capacity(3);
    for slot in wallet.slots() {
        let fp = slot.origin_fingerprint.to_string().to_uppercase();
        text.push_str(&format!("{fp}: {}\n", slot.xpub));
        keys.push(OutputKey {
            master_fingerprint: *slot.origin_fingerprint.as_bytes(),
            origin: origin_steps(slot.origin_path.as_ref()),
            parent_fingerprint: *slot.xpub.parent_fingerprint.as_bytes(),
            key: slot.xpub.public_key.serialize(),
            chain_code: *slot.xpub.chain_code.as_bytes(),
            testnet: wallet.network() != Network::Bitcoin,
            chain: 0,
        });
    }
    let output_cbor = wsh_sortedmulti_cbor(2, &keys)
        .map_err(|_| HardwareRefusal::Registration("the keys do not make a descriptor"))?;
    let [receive, change] = wallet.single_path_strings();
    Ok(Registration {
        name,
        text_file: text,
        descriptor_both: wallet.canonical_string().to_string(),
        descriptor_receive: receive,
        descriptor_change: change,
        output_cbor,
    })
}

/// AS-99: the receive address 0 of the wallet, which the signer's address
/// explorer must show.
pub fn first_address(wallet: &WalletDescription) -> Result<String, HardwareRefusal> {
    wallet
        .address(openbmt_core::wallet::Keychain::Receive, 0)
        .map(|a| a.to_string())
        .map_err(|_| HardwareRefusal::Registration("the wallet has no first address"))
}

/// AS-99: what was done about the first address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstAddressState {
    /// The owner compared it with the signer's and it matched.
    Checked,
    /// The owner chose not to check it now.
    Skipped,
}

/// AS-99: the record of the check, kept by the phone beside the wallet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstAddressRecord {
    /// The wallet's checksum (`WalletDescription::checksum32`).
    pub wallet: [u8; 32],
    /// What was done.
    pub state: FirstAddressState,
    /// When, in seconds.
    pub at: u64,
}

impl FirstAddressRecord {
    /// A record for `wallet`.
    pub fn new(wallet: &WalletDescription, state: FirstAddressState, at: u64) -> Self {
        Self {
            wallet: wallet.checksum32(),
            state,
            at,
        }
    }

    /// The framed bytes: the magic, the wallet, one byte of state (1 checked,
    /// 2 skipped) and the time.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(45);
        out.extend_from_slice(&MAGIC_FIRST_ADDRESS);
        out.extend_from_slice(&self.wallet);
        out.push(match self.state {
            FirstAddressState::Checked => 1,
            FirstAddressState::Skipped => 2,
        });
        out.extend_from_slice(&self.at.to_be_bytes());
        out
    }

    /// Read the record of `wallet`; anything else is refused.
    pub fn from_bytes(wallet: &WalletDescription, bytes: &[u8]) -> Result<Self, HardwareRefusal> {
        let bad = HardwareRefusal::Record("the record is not in the framed form");
        let (magic, rest) = bytes.split_first_chunk::<4>().ok_or_else(|| bad.clone())?;
        let (id, rest) = rest.split_first_chunk::<32>().ok_or_else(|| bad.clone())?;
        let (state, rest) = rest.split_first().ok_or_else(|| bad.clone())?;
        let at: [u8; 8] = rest.try_into().map_err(|_| bad.clone())?;
        if *magic != MAGIC_FIRST_ADDRESS {
            return Err(bad);
        }
        if *id != wallet.checksum32() {
            return Err(HardwareRefusal::Record("the record is for another wallet"));
        }
        let state = match state {
            1 => FirstAddressState::Checked,
            2 => FirstAddressState::Skipped,
            _ => return Err(bad),
        };
        Ok(Self {
            wallet: *id,
            state,
            at: u64::from_be_bytes(at),
        })
    }
}

impl Owner {
    /// The frozen spend of `number` in a session, and its request item's
    /// PSBT and labels. Only a session of spends qualifies.
    fn signer_item(
        &self,
        session: &[u8; 32],
        number: u32,
    ) -> Result<(Request, Frozen, Vec<u8>, Vec<openbmt_session::OutputLabel>), HardwareRefusal>
    {
        let open = self
            .state
            .sessions
            .iter()
            .find(|s| s.id == *session)
            // AS-101: a session of the shop's own wallet is not a signer's.
            .filter(|s| s.own.is_none())
            .ok_or(HardwareRefusal::NotASignerSpend)?;
        let request =
            Request::from_bytes(&open.request).map_err(|_| HardwareRefusal::NotASignerSpend)?;
        let frozen = open
            .frozen
            .iter()
            .find(|(n, _)| *n == number)
            .map(|(_, f)| f.clone())
            .ok_or(HardwareRefusal::NotASignerSpend)?;
        let Some(RequestItem {
            item: Item::Spend { psbt, labels },
            ..
        }) = request.item(number).cloned()
        else {
            return Err(HardwareRefusal::NotASignerSpend);
        };
        if !matches!(frozen, Frozen::Spend { .. }) {
            return Err(HardwareRefusal::NotASignerSpend);
        }
        Ok((request, frozen, psbt, labels))
    }

    pub(crate) fn summary(
        &self,
        psbt: &Psbt,
        payments: &[(bitcoin::ScriptBuf, u64)],
        change_position: Option<u32>,
    ) -> Result<SignerSummary, HardwareRefusal> {
        let network = self.wallet.network();
        let line =
            |script: &bitcoin::ScriptBuf, sats: u64| -> Result<SummaryLine, HardwareRefusal> {
                let address = Address::from_script(script, network)
                    .map_err(|_| HardwareRefusal::Key1("an output has no address".to_string()))?;
                Ok(SummaryLine {
                    address: address.to_string(),
                    sats,
                })
            };
        let mut paid = Vec::new();
        let mut change = None;
        for (i, out) in psbt.unsigned_tx.output.iter().enumerate() {
            let sats = out.value.to_sat();
            if change_position.and_then(|p| usize::try_from(p).ok()) == Some(i) {
                change = Some(line(&out.script_pubkey, sats)?);
            } else {
                paid.push(line(&out.script_pubkey, sats)?);
            }
        }
        let _ = payments;
        let spent: u64 = psbt
            .inputs
            .iter()
            .map(|i| i.witness_utxo.as_ref().map_or(0, |o| o.value.to_sat()))
            .sum();
        let sent: u64 = psbt
            .unsigned_tx
            .output
            .iter()
            .map(|o| o.value.to_sat())
            .sum();
        Ok(SignerSummary {
            payments: paid,
            change,
            fee: spent.saturating_sub(sent),
        })
    }

    fn file_of(&self, psbt_bytes: Vec<u8>, frozen: &Frozen) -> Result<SignerFile, HardwareRefusal> {
        let Frozen::Spend {
            txid,
            payments,
            change_position,
            ..
        } = frozen
        else {
            return Err(HardwareRefusal::NotASignerSpend);
        };
        let psbt = Psbt::deserialize(&psbt_bytes)
            .map_err(|_| HardwareRefusal::Key1("the file is not a PSBT".to_string()))?;
        let summary = self.summary(&psbt, payments, *change_position)?;
        let short: String = txid.to_string().chars().take(8).collect();
        Ok(SignerFile {
            psbt: psbt_bytes,
            txid: *txid,
            summary,
            file_name: format!("{short}.psbt"),
        })
    }

    /// AS-92: the file for the signer of an item that was exported already,
    /// to show again or carry again. Nothing is signed or changed.
    pub fn signer_file(
        &self,
        session: &[u8; 32],
        number: u32,
    ) -> Result<SignerFile, HardwareRefusal> {
        let (_, frozen, psbt, _) = self.signer_item(session, number)?;
        let parsed = Psbt::deserialize(&psbt).map_err(|_| HardwareRefusal::NotExported)?;
        if !is_exported(&parsed) {
            return Err(HardwareRefusal::NotExported);
        }
        self.file_of(psbt, &frozen)
    }

    /// AS-91: sign item `number` of the session `session` with key 1 and
    /// make the file for the hardware signer.
    ///
    /// The unsigned transaction must be the one that was frozen, with the
    /// frozen payments and change (AS-67). Key 1's signer reviews and signs
    /// it (its own rules apply), the file is made as SP-45 says, and the
    /// session keeps it as the file that was given: `persist` is handed the
    /// new queue and key 1's new state together, and nothing is changed
    /// unless it stores them. An item that was exported already gives the
    /// same file again and signs nothing.
    pub fn export_for_signer<E>(
        &mut self,
        session: &[u8; 32],
        number: u32,
        key1: &mut SignerState,
        key1_account: &Xpriv,
        now: u64,
        persist: impl FnOnce(&[u8], &[u8]) -> Result<(), E>,
    ) -> Result<SignerFile, OwnerError<E>> {
        let (request, frozen, psbt, labels) = self.signer_item(session, number)?;
        if let Ok(parsed) = Psbt::deserialize(&psbt) {
            if is_exported(&parsed) {
                return Ok(self.file_of(psbt, &frozen)?);
            }
        }
        let Frozen::Spend {
            txid,
            payments,
            change_index,
            change_position,
            ..
        } = &frozen
        else {
            return Err(HardwareRefusal::NotASignerSpend.into());
        };
        let key1_fail = |why: String| OwnerError::<E>::from(HardwareRefusal::Key1(why));
        if key1.wallet().canonical_string() != self.wallet.canonical_string() {
            return Err(Refusal::Key1Wallet.into());
        }
        let plan = Psbt::deserialize(&psbt)
            .map_err(|e| key1_fail(format!("the planned file is not a PSBT: {e}")))?;
        if plan.unsigned_tx.compute_txid() != *txid
            || !crate::take::outputs_are_frozen(
                self,
                &plan.unsigned_tx,
                payments,
                *change_index,
                *change_position,
            )
        {
            return Err(key1_fail(
                "the transaction is not the one that was frozen".to_string(),
            ));
        }
        let mut k1 = key1.clone();
        let review = k1
            .review(&psbt, now)
            .map_err(|e| key1_fail(e.to_string()))?;
        let signed = match k1.sign(review, key1_account, now, |_| Ok::<(), Infallible>(())) {
            Ok(s) => s,
            Err(SignError::Refused(r)) => return Err(key1_fail(r.to_string())),
            Err(SignError::Persist(never)) => match never {},
        };
        let signed_psbt = Psbt::deserialize(&signed.psbt)
            .map_err(|e| key1_fail(format!("key 1's file is not a PSBT: {e}")))?;
        let for_signer =
            psbt_for_signer(&self.wallet, &signed_psbt).map_err(|e| key1_fail(e.to_string()))?;
        let bytes = for_signer.serialize();
        // The session keeps the file that was given, in the item.
        let items: Vec<RequestItem> = request
            .items()
            .iter()
            .map(|ri| {
                if ri.number == number {
                    RequestItem {
                        number,
                        item: Item::Spend {
                            psbt: bytes.clone(),
                            labels: labels.clone(),
                        },
                    }
                } else {
                    ri.clone()
                }
            })
            .collect();
        let rebuilt = Request::new(&self.identity, *session, self.wallet_id, items)
            .map_err(|e| key1_fail(e.to_string()))?;
        let mut next = self.state.clone();
        if let Some(open) = next.sessions.iter_mut().find(|s| s.id == *session) {
            open.request = rebuilt.as_bytes().to_vec();
        }
        let file = self.file_of(bytes, &frozen)?;
        let owner_bytes = codec::encode(self, &next)?;
        if owner_bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(Refusal::SnapshotTooLarge.into());
        }
        persist(&owner_bytes, &k1.to_bytes()).map_err(OwnerError::Store)?;
        self.state = next;
        *key1 = k1;
        Ok(file)
    }

    /// AS-93 to AS-96: take what the hardware signer returned for item
    /// `number` of `session`.
    ///
    /// `returned` is read by [`read_returned`]. A PSBT must hold the
    /// unsigned transaction that was given (AS-94) and a valid signature of
    /// key 2 on every input (AS-95); it is joined with the file that was
    /// given and finished. A finished transaction must have the same ID and
    /// pass the check of SP-62. In both cases the outputs must be the frozen
    /// ones (AS-67). The item's entries go to "signed", the transaction waits
    /// in the outbox, and the item leaves the session, which ends with its last
    /// item (AS-88). `persist` is handed the new
    /// queue, and a refusal leaves everything as it was, so the right file
    /// can be brought again.
    pub fn take_from_signer<E>(
        &mut self,
        session: &[u8; 32],
        number: u32,
        returned: &[u8],
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<SignerTaken, OwnerError<E>> {
        let (_, frozen, sent_bytes, _) = self.signer_item(session, number)?;
        let sent = Psbt::deserialize(&sent_bytes).map_err(|_| HardwareRefusal::NotExported)?;
        if !is_exported(&sent) {
            return Err(HardwareRefusal::NotExported.into());
        }
        let Frozen::Spend {
            txid,
            refunds,
            cash_out,
            payments,
            change_index,
            change_position,
        } = &frozen
        else {
            return Err(HardwareRefusal::NotASignerSpend.into());
        };
        let finished = |why: String| OwnerError::<E>::from(HardwareRefusal::Finished(why));
        let tx = match read_returned(returned)? {
            Returned::Psbt(back) => {
                if back.unsigned_tx != sent.unsigned_tx {
                    return Err(HardwareRefusal::DifferentTransaction.into());
                }
                let both = take_signer_signatures(&self.wallet, &sent, &back, SIGNER_SLOT)
                    .map_err(|e| match e {
                        openbmt_spend::SignerFileError::NotSigned { .. }
                        | openbmt_spend::SignerFileError::Combine(_) => {
                            OwnerError::<E>::from(HardwareRefusal::NotSigned(e.to_string()))
                        }
                        other => finished(other.to_string()),
                    })?;
                finalize(&self.wallet, &both).map_err(|e| finished(e.to_string()))?
            }
            Returned::Transaction(tx) => {
                if tx.compute_txid() != *txid {
                    return Err(HardwareRefusal::DifferentTransaction.into());
                }
                check_finished(&self.wallet, &sent, &tx).map_err(|e| finished(e.to_string()))?;
                tx
            }
        };
        if tx.compute_txid() != *txid
            || !crate::take::outputs_are_frozen(
                self,
                &tx,
                payments,
                *change_index,
                *change_position,
            )
        {
            return Err(finished(
                "the finished transaction is not the one that was frozen".to_string(),
            ));
        }
        let fee = {
            let spent: u64 = sent
                .inputs
                .iter()
                .map(|i| i.witness_utxo.as_ref().map_or(0, |o| o.value.to_sat()))
                .sum();
            let out: u64 = tx.output.iter().map(|o| o.value.to_sat()).sum();
            spent.saturating_sub(out)
        };
        if self.state.outbox.len() >= MAX_OUTBOX {
            return Err(HardwareRefusal::OutboxFull.into());
        }
        let mut next = self.state.clone();
        // AS-96: only this item leaves the session; the others stay.
        self.remove_item(&mut next, session, number)?;
        let id = next.take_id()?;
        for n in refunds.iter().copied().chain(*cash_out) {
            if let Some(stage) = next.stage_mut(n) {
                *stage = Stage::Signed { txid: *txid };
            }
        }
        next.outbox.push(Deliverable {
            id,
            what: Delivery::Transaction {
                txid: *txid,
                tx: bitcoin::consensus::serialize(&tx),
                refunds: refunds.clone(),
                cash_out: *cash_out,
            },
        });
        let owner_bytes = codec::encode(self, &next)?;
        if owner_bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(Refusal::SnapshotTooLarge.into());
        }
        persist(&owner_bytes).map_err(OwnerError::Store)?;
        self.state = next;
        Ok(SignerTaken {
            id,
            txid: *txid,
            fee,
        })
    }
}

/// Whether the file holds a signature on every input already: key 1's, put
/// there when it was exported.
pub(crate) fn is_exported(psbt: &Psbt) -> bool {
    !psbt.inputs.is_empty() && psbt.inputs.iter().all(|i| !i.partial_sigs.is_empty())
}
