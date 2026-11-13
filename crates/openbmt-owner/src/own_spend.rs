//! Spends from a shop's own wallet (`docs/SESSION-RULES.md` section 6,
//! AS-100 to AS-107): the refunds, cash-outs and merges of a shop with no
//! Key app are frozen here and signed in the shop's own wallet, a wallet with
//! one key or a 2-of-3 wallet whose keys the owner holds.
//!
//! Nothing on the owner phone can sign. [`Owner::build_own_spend`] freezes
//! the spends the owner picked into a session of their own, [`Owner::own_file`]
//! gives the file of one item for the wallet and from then on its transaction
//! is watched (AS-105), [`Owner::take_own`] takes back what the wallet made
//! and [`Owner::abandon_session`](crate::Owner::abandon_session) gives a
//! session up. The file and the finishing reuse the hardware signer's path
//! (`hardware`): `psbt_for_signer`, `finalize`, `check_finished` and
//! `transaction_of_final_file` of the spend crate.
//!
//! Every refusal here has words of its own, listed by
//! [`all_own_spend_words`] for the checks of SC-12.

use std::collections::BTreeSet;
use std::fmt;

use bitcoin::consensus::serialize;
use bitcoin::psbt::Psbt;
use bitcoin::Txid;
use openbmt_core::identity::tagged_hash;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::wallet_single::ImportedWallet;
use openbmt_session::{Item, Request, RequestItem};
use openbmt_spend::{
    check_finished, finalize, psbt_for_signer, take_own_signatures, Coin, SignerFileError,
};

use crate::error::{OwnerError, Refusal};
use crate::hardware::{read_returned, HardwareRefusal, Returned, SignerSummary};
use crate::state::{Frozen, OpenSession, Owner, State};
use crate::types::{Deliverable, Delivery, Skipped, Stage};
use crate::MAX_OUTBOX;

/// AS-101: the tag of the hash that gives a session of the spends of a shop's
/// own wallet its ID, from the frozen items and the caller's random bytes.
pub const TAG_OWN_SPEND: &str = "OpenBMT/own-spend/v1";

/// Why a step of a spend from the shop's own wallet was refused. Nothing was
/// changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnSpendRefusal {
    /// AS-100: the shop has a Key app, so these steps are not its own.
    NotOwnWallet,
    /// AS-101: no refund or cash-out was picked.
    NothingPicked,
    /// AS-101: a picked entry is not a refund or a cash-out in the queue.
    NotQueued(u32),
    /// AS-102: the session or the item is not a spend of the shop's own
    /// wallet.
    NotAnOwnSpend,
    /// AS-102: the file of the item could not be made. The text says why.
    File(String),
    /// AS-104: the file of the item was not given to the wallet yet.
    NotFiled,
    /// AS-103: what came back is not a PSBT or a transaction in a form that
    /// is read. The text says what is wrong.
    Unreadable(&'static str),
    /// AS-103: what came back holds another transaction than the file given.
    DifferentTransaction,
    /// AS-103: the wallet did not sign: a signature is missing on an input,
    /// is of a key that is not the wallet's, or does not verify.
    NotSigned(String),
    /// AS-103: the finished transaction does not pass the checks.
    Finished(String),
    /// AS-104: too many finished objects are waiting.
    OutboxFull,
}

impl fmt::Display for OwnSpendRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotOwnWallet => write!(f, "AS-100: this shop has a Key app"),
            Self::NothingPicked => write!(f, "AS-101: no payment was picked"),
            Self::NotQueued(n) => write!(f, "AS-101: entry {n} is not a queued refund or cash-out"),
            Self::NotAnOwnSpend => write!(f, "AS-102: this is not a payment to sign in the wallet"),
            Self::File(why) => write!(f, "AS-102: the file cannot be made: {why}"),
            Self::NotFiled => write!(f, "AS-104: the file was not given to the wallet yet"),
            Self::Unreadable(why) => write!(f, "AS-103: what came back cannot be read: {why}"),
            Self::DifferentTransaction => write!(
                f,
                "AS-103: what came back holds another transaction than the one given"
            ),
            Self::NotSigned(why) => write!(f, "AS-103: the wallet did not sign: {why}"),
            Self::Finished(why) => write!(f, "AS-103: {why}"),
            Self::OutboxFull => write!(
                f,
                "AS-104: too many finished objects are waiting to be delivered"
            ),
        }
    }
}

impl std::error::Error for OwnSpendRefusal {}

impl From<OwnSpendRefusal> for Refusal {
    fn from(r: OwnSpendRefusal) -> Self {
        Self::OwnSpend(r)
    }
}

impl<E> From<OwnSpendRefusal> for OwnerError<E> {
    fn from(r: OwnSpendRefusal) -> Self {
        Self::Refused(Refusal::OwnSpend(r))
    }
}

fn stop(heading: &str, what: &str, next: &str, code: &'static str) -> Words {
    Words::new(Tone::Stop, heading, what, Who::Owner, next).code(code)
}

/// AS-100: the words of a queue call that needs the Key app, in a shop whose
/// money goes to its own wallet (`Refusal::NoKeyApp`).
pub fn no_key_app_words() -> Words {
    stop(
        "Sign this in your wallet",
        "This shop has no Key app. Its refunds, cash-outs and merges are signed in the shop's own wallet.",
        "Nothing was changed. Open Send from your wallet and pick the payment there.",
        "AS-100",
    )
}

impl OwnSpendRefusal {
    /// What the owner is told, with the library's own text as the detail
    /// where it has one.
    pub fn words(&self) -> Words {
        match self {
            Self::NotOwnWallet => stop(
                "This shop uses the Key app",
                "The payments of this shop are signed on the Key app, not in a wallet of the owner's.",
                "Nothing was changed. Send the payment from the queue as for the Key app.",
                "AS-100",
            ),
            Self::NothingPicked => stop(
                "Nothing picked",
                "No refund or cash-out was picked to send.",
                "Nothing was changed. Pick at least one payment from the list.",
                "AS-101",
            ),
            Self::NotQueued(_) => stop(
                "No longer waiting",
                "A picked payment is no longer a refund or cash-out waiting in the queue.",
                "Nothing was changed. Go back to the list, which shows what still waits.",
                "AS-101",
            ),
            Self::NotAnOwnSpend => stop(
                "Not for your wallet",
                "This item is not a payment to sign in the shop's own wallet.",
                "Nothing was changed. Pick a payment from Send from your wallet.",
                "AS-102",
            ),
            Self::File(why) => stop(
                "Cannot make the file",
                "The file for the wallet cannot be made from this payment.",
                "Nothing was changed. Report this to the OpenBMT project with the details.",
                "AS-102",
            )
            .detail(why.clone()),
            Self::NotFiled => stop(
                "Nothing given to the wallet",
                "The file of this payment has not been given to the wallet yet.",
                "Nothing was changed. Give the file to the wallet first, then bring back what it signs.",
                "AS-104",
            ),
            Self::Unreadable(why) => stop(
                "That is not from the wallet",
                "What was read is not a signing file or a finished payment in a form this phone takes.",
                "Nothing was changed. Scan or open what the wallet made again, or look in the wallet's help for how it gives back a signed payment.",
                "AS-103",
            )
            .detail((*why).to_string()),
            Self::DifferentTransaction => stop(
                "Another payment came back",
                "What came back is a different payment from the one given to the wallet.",
                "Nothing was changed. Do not send it. Sign the payment shown here in the wallet, and bring that back.",
                "AS-103",
            ),
            Self::NotSigned(why) => stop(
                "The wallet did not sign",
                "What came back is missing a valid signature of this shop's keys on an input.",
                "Nothing was changed. Check that the wallet holds the shop's keys, then sign again. A 2-of-3 wallet needs two of them.",
                "AS-103",
            )
            .detail(why.clone()),
            Self::Finished(why) => stop(
                "The payment failed a check",
                "The finished payment is not exactly the one prepared on this phone, or its signatures do not check out.",
                "Nothing was changed and nothing was sent. Sign again in the wallet and bring that back.",
                "AS-103",
            )
            .detail(why.clone()),
            Self::OutboxFull => stop(
                "Too many payments waiting",
                "Too many finished payments wait to go out to the network, so this one was not taken.",
                "Nothing was changed. Let the waiting payments go out first, then bring this back.",
                "AS-104",
            ),
        }
    }
}

/// The ways a refusal of this module can come, with a value of each, for the
/// words of every state to be listed.
pub fn all_refusals() -> Vec<(&'static str, OwnSpendRefusal)> {
    vec![
        ("NotOwnWallet", OwnSpendRefusal::NotOwnWallet),
        ("NothingPicked", OwnSpendRefusal::NothingPicked),
        ("NotQueued", OwnSpendRefusal::NotQueued(1)),
        ("NotAnOwnSpend", OwnSpendRefusal::NotAnOwnSpend),
        (
            "File",
            OwnSpendRefusal::File("the wallet has no change address".to_string()),
        ),
        ("NotFiled", OwnSpendRefusal::NotFiled),
        (
            "Unreadable",
            OwnSpendRefusal::Unreadable("it is not a PSBT or a transaction"),
        ),
        (
            "DifferentTransaction",
            OwnSpendRefusal::DifferentTransaction,
        ),
        (
            "NotSigned",
            OwnSpendRefusal::NotSigned("input 0 has no signature".to_string()),
        ),
        (
            "Finished",
            OwnSpendRefusal::Finished("the witness of input 0 does not verify".to_string()),
        ),
        ("OutboxFull", OwnSpendRefusal::OutboxFull),
    ]
}

/// AS-102: the words beside the file for the wallet: what its screen shows,
/// and that the payment must not be approved there unless every line
/// matches.
pub fn compare_words() -> Words {
    Words::new(
        Tone::Info,
        "Check it in your wallet",
        "The wallet shows each address, each amount and the fee; compare every one with this screen.",
        Who::Owner,
        "Approve in the wallet only if they all match.",
    )
    .code("AS-102")
}

/// Every state of this module that has words, by name: the refusals, the
/// words beside the file and the refusal of AS-100.
pub fn all_own_spend_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = all_refusals()
        .into_iter()
        .map(|(n, r)| (format!("Refusal::OwnSpend/{n}"), r.words()))
        .collect();
    all.push(("Refusal::NoKeyApp".to_string(), no_key_app_words()));
    all.push(("OwnSpendCompare".to_string(), compare_words()));
    all
}

/// AS-101: what `build_own_spend` froze.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnBuilt {
    /// The session's ID, or `None` when nothing was frozen (and nothing was
    /// stored).
    pub session: Option<[u8; 32]>,
    /// The items, one for each transaction, in order.
    pub items: Vec<OwnItem>,
    /// Entries that stayed queued, with the reason.
    pub skipped: Vec<Skipped>,
}

/// One item of a session of a shop's own wallet: one transaction (AS-101).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnItem {
    /// The item's number in its session.
    pub number: u32,
    /// The transaction's ID, which signatures do not change (AS-105).
    pub txid: Txid,
    /// The refunds the transaction pays.
    pub refunds: Vec<u32>,
    /// The cash-out it pays, if it is one.
    pub cash_out: Option<u32>,
    /// Whether the file was given to the wallet, so that the transaction is
    /// watched (AS-105).
    pub filed: bool,
    /// What the wallet's screen must show (AS-102).
    pub summary: SignerSummary,
}

/// AS-102: the file for the shop's wallet and what to compare on its screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnFile {
    /// The PSBT, binary BIP 174 version 0: the file of SP-82 for a wallet
    /// with one key, the file of SP-45 with no signature for a 2-of-3
    /// wallet.
    pub psbt: Vec<u8>,
    /// The transaction's ID.
    pub txid: Txid,
    /// What the wallet's screen must show.
    pub summary: SignerSummary,
    /// A name for a file: `<first 8 characters of the ID>.psbt`.
    pub file_name: String,
}

/// AS-104: what was taken from the wallet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnTaken {
    /// The finished transaction's number in the outbox.
    pub id: u32,
    /// Its ID.
    pub txid: Txid,
    /// Its fee in satoshis.
    pub fee: u64,
}

/// One item of a session of the shop's own wallet, as the steps need it.
struct Found {
    frozen: Frozen,
    psbt: Vec<u8>,
    filed: bool,
}

impl Owner {
    /// AS-101: freeze the queued refunds and cash-outs the owner picked into
    /// a session for the shop's own wallet.
    ///
    /// `picked` holds the numbers of the entries. `coins`, `fee_rate`,
    /// `change_index` and `lock_height` are as for
    /// [`Owner::build_request`]. `random` is 32 bytes from the caller's
    /// random source: they seed the change positions and, hashed with the
    /// frozen items under [`TAG_OWN_SPEND`], give the session its ID. There
    /// is no request for a Key app and no health check; the entries wait "in
    /// session" with that ID and nothing is signed. An entry that does not
    /// fit stays queued and is reported in [`OwnBuilt::skipped`]; if nothing
    /// fits, nothing is stored.
    #[allow(clippy::too_many_arguments)]
    pub fn build_own_spend<E>(
        &mut self,
        coins: &[Coin],
        picked: &[u32],
        fee_rate: u64,
        change_index: u32,
        lock_height: u32,
        now: u64,
        random: &[u8; 32],
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<OwnBuilt, OwnerError<E>> {
        if !self.no_key_app {
            return Err(OwnSpendRefusal::NotOwnWallet.into());
        }
        if picked.is_empty() {
            return Err(OwnSpendRefusal::NothingPicked.into());
        }
        let picked: BTreeSet<u32> = picked.iter().copied().collect();
        for n in &picked {
            let queued = self
                .state
                .refunds
                .iter()
                .find(|r| r.id == *n)
                .map(|r| &r.stage)
                .or_else(|| {
                    self.state
                        .cash_outs
                        .iter()
                        .find(|c| c.id == *n)
                        .map(|c| &c.stage)
                })
                .is_some_and(|stage| *stage == Stage::Queued);
            if !queued {
                return Err(OwnSpendRefusal::NotQueued(*n).into());
            }
        }
        let mut seeds = [0u8; 96];
        for chunk in seeds.chunks_mut(32) {
            chunk.copy_from_slice(random);
        }
        let (built, session) = self.build_inner(
            coins,
            fee_rate,
            change_index,
            lock_height,
            now,
            &seeds,
            false,
            Some(&picked),
            persist,
        )?;
        let items = match &session {
            Some(id) => self.own_items(id)?,
            None => Vec::new(),
        };
        Ok(OwnBuilt {
            session,
            items,
            skipped: built.skipped,
        })
    }

    /// AS-101: the tail of the build for a shop's own wallet: the items made
    /// become one session, whose ID is a tagged hash of them and of the
    /// caller's random bytes.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn freeze_own<E>(
        &mut self,
        mut next: State,
        items: Vec<(Item, Frozen)>,
        mut skipped: Vec<Skipped>,
        picked: &BTreeSet<u32>,
        random: &[u8; 96],
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(crate::types::Built, Option<[u8; 32]>), OwnerError<E>> {
        let frozen_ids: BTreeSet<u32> = items
            .iter()
            .flat_map(|(_, f)| match f {
                Frozen::Spend {
                    refunds, cash_out, ..
                } => refunds.iter().copied().chain(*cash_out).collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect();
        for n in picked {
            if !frozen_ids.contains(n) && !skipped.iter().any(|s| s.id == *n) {
                skipped.push(Skipped {
                    id: *n,
                    reason: "AS-53: the sale has not reached the refund depth, or its facts do \
                             not cover the refunds counted against it"
                        .to_string(),
                });
            }
        }
        let none = |skipped: Vec<Skipped>| crate::types::Built {
            session_id: [0; 32],
            request: Vec::new(),
            skipped,
        };
        if items.is_empty() {
            return Ok((none(skipped), None));
        }
        let mut hashed: Vec<u8> = random.get(..32).unwrap_or(&[]).to_vec();
        let mut request_items: Vec<RequestItem> = Vec::with_capacity(items.len());
        let mut frozen: Vec<(u32, Frozen)> = Vec::with_capacity(items.len());
        for (n, (item, f)) in items.into_iter().enumerate() {
            let number = u32::try_from(n).unwrap_or(u32::MAX);
            hashed.extend_from_slice(&number.to_le_bytes());
            if let (Item::Spend { psbt, labels }, Frozen::Spend { txid, .. }) = (&item, &f) {
                hashed.extend_from_slice(txid.as_ref());
                hashed.extend_from_slice(&u64::try_from(psbt.len()).unwrap_or(0).to_le_bytes());
                hashed.extend_from_slice(psbt);
                hashed.extend_from_slice(&u32::try_from(labels.len()).unwrap_or(0).to_le_bytes());
            }
            request_items.push(RequestItem { number, item });
            frozen.push((number, f));
        }
        let id = tagged_hash(TAG_OWN_SPEND, &hashed);
        if next.sessions.iter().any(|s| s.id == id) {
            return Err(Refusal::SessionIdUsed.into());
        }
        let request = Request::new(&self.identity, id, self.wallet_id, request_items)
            .map_err(Refusal::Encode)?;
        for (_, f) in &frozen {
            if let Frozen::Spend {
                refunds, cash_out, ..
            } = f
            {
                for n in refunds.iter().copied().chain(*cash_out) {
                    if let Some(stage) = next.stage_mut(n) {
                        *stage = Stage::InSession { session: id };
                    }
                }
            }
        }
        next.sessions.push(OpenSession {
            id,
            created_at: now,
            request: request.as_bytes().to_vec(),
            frozen,
            own: Some(BTreeSet::new()),
        });
        self.commit(next, persist)?;
        Ok((none(skipped), Some(id)))
    }

    /// The items of the session `session` of the shop's own wallet, with
    /// what the wallet's screen must show for each (AS-102).
    pub fn own_items(&self, session: &[u8; 32]) -> Result<Vec<OwnItem>, OwnSpendRefusal> {
        let open = self
            .state
            .sessions
            .iter()
            .find(|s| s.id == *session)
            .filter(|s| s.own.is_some())
            .ok_or(OwnSpendRefusal::NotAnOwnSpend)?;
        let filed = open.own.as_ref();
        let request =
            Request::from_bytes(&open.request).map_err(|_| OwnSpendRefusal::NotAnOwnSpend)?;
        let mut out = Vec::with_capacity(open.frozen.len());
        for (number, f) in &open.frozen {
            let Frozen::Spend {
                txid,
                refunds,
                cash_out,
                payments,
                change_position,
                ..
            } = f
            else {
                return Err(OwnSpendRefusal::NotAnOwnSpend);
            };
            let Some(RequestItem {
                item: Item::Spend { psbt, .. },
                ..
            }) = request.item(*number)
            else {
                return Err(OwnSpendRefusal::NotAnOwnSpend);
            };
            let plan = Psbt::deserialize(psbt).map_err(|e| OwnSpendRefusal::File(e.to_string()))?;
            let summary = self
                .summary(&plan, payments, *change_position)
                .map_err(|e| OwnSpendRefusal::File(e.to_string()))?;
            out.push(OwnItem {
                number: *number,
                txid: *txid,
                refunds: refunds.clone(),
                cash_out: *cash_out,
                filed: filed.is_some_and(|set| set.contains(number)),
                summary,
            });
        }
        Ok(out)
    }

    /// The IDs of the open sessions of the shop's own wallet.
    pub fn own_sessions(&self) -> Vec<[u8; 32]> {
        self.state
            .sessions
            .iter()
            .filter(|s| s.own.is_some())
            .map(|s| s.id)
            .collect()
    }

    /// AS-102, AS-103: the frozen item `number` of an own session, its plan
    /// file as frozen, and whether its file was given.
    fn own_item(&self, session: &[u8; 32], number: u32) -> Result<Found, OwnSpendRefusal> {
        let open = self
            .state
            .sessions
            .iter()
            .find(|s| s.id == *session)
            .filter(|s| s.own.is_some())
            .ok_or(OwnSpendRefusal::NotAnOwnSpend)?;
        let request =
            Request::from_bytes(&open.request).map_err(|_| OwnSpendRefusal::NotAnOwnSpend)?;
        let frozen = open
            .frozen
            .iter()
            .find(|(n, _)| *n == number)
            .map(|(_, f)| f.clone())
            .filter(|f| matches!(f, Frozen::Spend { .. }))
            .ok_or(OwnSpendRefusal::NotAnOwnSpend)?;
        let Some(RequestItem {
            item: Item::Spend { psbt, .. },
            ..
        }) = request.item(number).cloned()
        else {
            return Err(OwnSpendRefusal::NotAnOwnSpend);
        };
        Ok(Found {
            frozen,
            psbt,
            filed: open.own.as_ref().is_some_and(|set| set.contains(&number)),
        })
    }

    /// AS-102: the file of the frozen plan for the wallet: the plan's own
    /// file for a wallet with one key (SP-82), and the file of SP-45 with no
    /// signature for a 2-of-3 wallet. The same plan gives the same bytes.
    fn wallet_file(&self, plan: &[u8]) -> Result<Vec<u8>, OwnSpendRefusal> {
        let parsed = Psbt::deserialize(plan).map_err(|e| OwnSpendRefusal::File(e.to_string()))?;
        match &self.wallet {
            ImportedWallet::Single(_) => Ok(parsed.serialize()),
            ImportedWallet::Multisig(_) => psbt_for_signer(&self.wallet, &parsed)
                .map(|p| p.serialize())
                .map_err(|e| OwnSpendRefusal::File(e.to_string())),
        }
    }

    /// AS-102, AS-105: the file of item `number` of the own session
    /// `session`, for the shop's wallet. From the first call the item's
    /// transaction is watched: `persist` is handed the new queue, and
    /// nothing changes unless it stores it. An item that was filed already
    /// gives the same file again and stores nothing.
    pub fn own_file<E>(
        &mut self,
        session: &[u8; 32],
        number: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<OwnFile, OwnerError<E>> {
        let found = self.own_item(session, number)?;
        let Frozen::Spend {
            txid,
            payments,
            change_position,
            ..
        } = &found.frozen
        else {
            return Err(OwnSpendRefusal::NotAnOwnSpend.into());
        };
        let bytes = self.wallet_file(&found.psbt)?;
        let parsed = Psbt::deserialize(&bytes).map_err(|e| OwnSpendRefusal::File(e.to_string()))?;
        let summary = self
            .summary(&parsed, payments, *change_position)
            .map_err(|e| OwnSpendRefusal::File(e.to_string()))?;
        let short: String = txid.to_string().chars().take(8).collect();
        let file = OwnFile {
            psbt: bytes,
            txid: *txid,
            summary,
            file_name: format!("{short}.psbt"),
        };
        if found.filed {
            return Ok(file);
        }
        let mut next = self.state.clone();
        if let Some(set) = next
            .sessions
            .iter_mut()
            .find(|s| s.id == *session)
            .and_then(|s| s.own.as_mut())
        {
            set.insert(number);
        }
        self.commit(next, persist)?;
        Ok(file)
    }

    /// AS-103, AS-104: take what the shop's wallet returned for item
    /// `number` of the own session `session`.
    ///
    /// `returned` is read by [`read_returned`]. A PSBT must hold the unsigned
    /// transaction that was given and, on every input, a valid signature of
    /// the wallet's key (one key) or valid signatures of two different keys
    /// of the wallet (2-of-3); it is joined with the file that was given and
    /// finished. A finished transaction must have the same ID and pass the
    /// check of SP-86 or SP-47. In both cases the outputs must be the frozen
    /// ones (AS-67). The item's entries go to "signed", the transaction waits
    /// in the outbox, and the item leaves the session, which ends with its
    /// last item (AS-88). The queue is stored before the transaction is given
    /// to the outbox; a refusal or a failed store changes nothing, so the
    /// right file can be brought again.
    pub fn take_own<E>(
        &mut self,
        session: &[u8; 32],
        number: u32,
        returned: &[u8],
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<OwnTaken, OwnerError<E>> {
        let found = self.own_item(session, number)?;
        if !found.filed {
            return Err(OwnSpendRefusal::NotFiled.into());
        }
        let sent = Psbt::deserialize(&self.wallet_file(&found.psbt)?)
            .map_err(|e| OwnSpendRefusal::File(e.to_string()))?;
        let Frozen::Spend {
            txid,
            refunds,
            cash_out,
            payments,
            change_index,
            change_position,
        } = &found.frozen
        else {
            return Err(OwnSpendRefusal::NotAnOwnSpend.into());
        };
        let finished = |why: String| OwnerError::<E>::from(OwnSpendRefusal::Finished(why));
        let read = read_returned(returned).map_err(|e| match e {
            HardwareRefusal::Unreadable(why) => OwnSpendRefusal::Unreadable(why),
            other => OwnSpendRefusal::File(other.to_string()),
        })?;
        let tx = match read {
            Returned::Psbt(back) => {
                if back.unsigned_tx != sent.unsigned_tx {
                    return Err(OwnSpendRefusal::DifferentTransaction.into());
                }
                let both =
                    take_own_signatures(&self.wallet, &sent, &back).map_err(|e| match e {
                        SignerFileError::NotSigned { .. } | SignerFileError::Combine(_) => {
                            OwnerError::<E>::from(OwnSpendRefusal::NotSigned(e.to_string()))
                        }
                        other => finished(other.to_string()),
                    })?;
                // Fewer than two different keys, or a bad one, is a wallet
                // that did not sign; anything else is a failed check.
                finalize(&self.wallet, &both)
                    .map_err(|e| OwnerError::<E>::from(OwnSpendRefusal::NotSigned(e.to_string())))?
            }
            Returned::Transaction(tx) => {
                if tx.compute_txid() != *txid {
                    return Err(OwnSpendRefusal::DifferentTransaction.into());
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
            return Err(OwnSpendRefusal::OutboxFull.into());
        }
        let mut next = self.state.clone();
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
                tx: serialize(&tx),
                refunds: refunds.clone(),
                cash_out: *cash_out,
            },
        });
        self.commit(next, persist)?;
        Ok(OwnTaken {
            id,
            txid: *txid,
            fee,
        })
    }
}
