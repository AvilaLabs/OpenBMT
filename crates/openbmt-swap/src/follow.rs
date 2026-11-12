//! Following one swap from the request to the claim by polling (SW-70 to
//! SW-74).
//!
//! The machine takes what the caller read (a status, a lockup transaction,
//! the height of the chain) and says what to do next. It never believes the
//! service about money: a lockup is believed only after SW-30 has checked
//! the transaction's bytes, and the swap is paid only when the caller says
//! the shop's own claim transaction was seen confirmed. The service's status
//! decides when to look, nothing else. Every change makes a new value; the
//! old one is not touched.
//!
//! Layout of the saved machine, all numbers big-endian: magic `OBM'`,
//! version 1, stage (1), the stage's fields, the length of the swap's state
//! file (2) and that file (`SwapState::encode`). The stage's fields: for
//! locked, the lockup's transaction id (32), output index (4), value (8) and
//! flags (1: bit 0 confirmed, bit 1 signals replacement); for paid, the
//! height of the block (4); for failed, the failure (1); none otherwise.

use bitcoin::hashes::Hash;
use bitcoin::{OutPoint, Transaction, Txid};

use crate::claim::build_claim;
use crate::error::Refused;
use crate::lockup::{check_lockup, LockupSeen, Seen};
use crate::state::{Phase, SwapState};
use crate::status::{Status, StatusKind};
use crate::{POLL_LOCKED_SECS, POLL_WAITING_SECS};

/// SW-73: the first four bytes of a saved machine.
pub const MAGIC_FOLLOW: [u8; 4] = *b"OBM'";
/// SW-73: the version of the layout.
const FOLLOW_VERSION: u8 = 1;

/// Why a swap will not be paid. All of them are final.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The invoice ran out before a lockup was seen.
    InvoiceExpired,
    /// The service said it could not make the lockup.
    LockupFailed,
    /// The service said the swap ran out.
    SwapExpired,
    /// The service said it took its lockup back.
    Refunded,
    /// The chain reached the timeout before the claim was sent.
    TimedOut,
    /// The lockup the service pointed to is not what was promised (SW-30).
    LockupRefused,
}

impl Failure {
    fn to_byte(self) -> u8 {
        match self {
            Self::InvoiceExpired => 1,
            Self::LockupFailed => 2,
            Self::SwapExpired => 3,
            Self::Refunded => 4,
            Self::TimedOut => 5,
            Self::LockupRefused => 6,
        }
    }

    fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::InvoiceExpired,
            2 => Self::LockupFailed,
            3 => Self::SwapExpired,
            4 => Self::Refunded,
            5 => Self::TimedOut,
            6 => Self::LockupRefused,
            _ => return None,
        })
    }
}

/// How far the machine has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The swap was made; no lockup is known.
    Created,
    /// The service says the lockup exists; its bytes have not been checked.
    Announced,
    /// A lockup that passed SW-30 is known.
    Locked(LockupSeen),
    /// The claim was built and handed over, and is not yet seen confirmed.
    ClaimSent,
    /// The claim was seen confirmed, in a block of this height. Paid.
    Paid {
        /// The height of the block.
        height: u32,
    },
    /// The swap will not be paid.
    Failed(Failure),
}

/// What the caller knows now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ctx {
    /// The time, in seconds since 1970.
    pub now_secs: u64,
    /// The height of the chain tip.
    pub tip_height: u32,
    /// Whether the shop sends the claim for a lockup that is not yet in a
    /// block (the service can still replace it; SW-31). The shop's rule.
    pub accept_unconfirmed: bool,
}

/// What to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Nothing; ask for the status again after this many seconds.
    Wait {
        /// Seconds.
        poll_after_secs: u32,
    },
    /// The service says there is a lockup: find it on chain and pass its
    /// bytes to [`ReverseSwap::on_lockup`].
    CheckLockup,
    /// Build the claim of this lockup ([`ReverseSwap::claim`]).
    BuildClaim {
        /// The lockup that passed SW-30.
        lockup: LockupSeen,
    },
    /// The claim is out: watch for it in a block and report it
    /// ([`ReverseSwap::claim_confirmed`]).
    WatchClaim {
        /// The claim's transaction id.
        txid: Txid,
    },
    /// Paid: the shop's claim is confirmed.
    Paid {
        /// The claim's transaction id.
        txid: Txid,
    },
    /// The swap will not be paid.
    Failed(Failure),
}

/// One swap, followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReverseSwap {
    state: SwapState,
    stage: Stage,
}

impl ReverseSwap {
    /// A swap that was made and checked, at the start. A state that already
    /// holds a claim starts at the claim.
    pub fn new(state: SwapState) -> Self {
        let stage = match state.phase() {
            Phase::Waiting => Stage::Created,
            Phase::Claimed(_) => Stage::ClaimSent,
        };
        Self { state, stage }
    }

    /// The swap's state file.
    pub fn state(&self) -> &SwapState {
        &self.state
    }

    /// How far the machine has got.
    pub fn stage(&self) -> Stage {
        self.stage
    }

    fn with(&self, stage: Stage) -> Self {
        Self {
            state: self.state.clone(),
            stage,
        }
    }

    /// What to do now, and the machine with any change the clock and the
    /// chain force: past the timeout before the claim is sent, or past the
    /// invoice's end with no lockup, the swap has failed.
    pub fn settle(&self, ctx: &Ctx) -> (Self, Next) {
        let late = ctx.tip_height >= self.state.timeout();
        let fail = |f: Failure| (self.with(Stage::Failed(f)), Next::Failed(f));
        match self.stage {
            Stage::Created if late => fail(Failure::TimedOut),
            Stage::Created if ctx.now_secs > self.state.invoice_expires_at() => {
                fail(Failure::InvoiceExpired)
            }
            Stage::Created => (
                self.clone(),
                Next::Wait {
                    poll_after_secs: POLL_WAITING_SECS,
                },
            ),
            Stage::Announced if late => fail(Failure::TimedOut),
            Stage::Announced => (self.clone(), Next::CheckLockup),
            Stage::Locked(_) if late => fail(Failure::TimedOut),
            Stage::Locked(lockup) if lockup.confirmed || ctx.accept_unconfirmed => {
                (self.clone(), Next::BuildClaim { lockup })
            }
            Stage::Locked(_) => (
                self.clone(),
                Next::Wait {
                    poll_after_secs: POLL_LOCKED_SECS,
                },
            ),
            Stage::ClaimSent => match self.state.phase() {
                Phase::Claimed(txid) => (self.clone(), Next::WatchClaim { txid }),
                Phase::Waiting => fail(Failure::LockupRefused),
            },
            Stage::Paid { .. } => match self.state.phase() {
                Phase::Claimed(txid) => (self.clone(), Next::Paid { txid }),
                Phase::Waiting => fail(Failure::LockupRefused),
            },
            Stage::Failed(f) => (self.clone(), Next::Failed(f)),
        }
    }

    /// SW-70, SW-71, SW-72: a status reply was read. It moves the machine
    /// only toward looking: a status that names a lockup makes the machine
    /// ask for it, and the bytes the status carries are checked by SW-30 as
    /// if the caller had found them, and always as not yet in a block,
    /// whatever the service says. A status that says the swap has failed
    /// ends it, so that the preimage is not shown for a swap the service has
    /// given up. After the claim is sent, a status changes nothing.
    pub fn on_status(&self, status: &Status, ctx: &Ctx) -> (Self, Next) {
        if !matches!(
            self.stage,
            Stage::Created | Stage::Announced | Stage::Locked(_)
        ) {
            return self.settle(ctx);
        }
        let next_stage = match status.kind() {
            StatusKind::SwapCreated | StatusKind::InvoiceSettled | StatusKind::Unknown(_) => {
                self.stage
            }
            StatusKind::InvoiceExpired => Stage::Failed(Failure::InvoiceExpired),
            StatusKind::TransactionFailed => Stage::Failed(Failure::LockupFailed),
            StatusKind::SwapExpired => Stage::Failed(Failure::SwapExpired),
            StatusKind::TransactionRefunded => Stage::Failed(Failure::Refunded),
            StatusKind::TransactionMempool | StatusKind::TransactionConfirmed => {
                match status.transaction() {
                    None => match self.stage {
                        Stage::Created => Stage::Announced,
                        other => other,
                    },
                    Some(tx) => match self.read_lockup(tx, Seen::Mempool) {
                        // What the caller already saw in a block is kept.
                        Ok(l) => match self.stage {
                            Stage::Locked(old) if old.outpoint == l.outpoint => self.stage,
                            _ => Stage::Locked(l),
                        },
                        Err(_) => Stage::Failed(Failure::LockupRefused),
                    },
                }
            }
        };
        self.with(next_stage).settle(ctx)
    }

    /// SW-30, SW-72: the caller found a transaction that should be the
    /// lockup, and where. It is checked against the lockup address and the
    /// amount the service promised; a transaction that fails changes nothing.
    pub fn on_lockup(
        &self,
        tx: &Transaction,
        seen: Seen,
        ctx: &Ctx,
    ) -> Result<(Self, Next), Refused> {
        if !matches!(
            self.stage,
            Stage::Created | Stage::Announced | Stage::Locked(_)
        ) {
            return Err(Refused::NotReady("no lockup is expected"));
        }
        let lockup = self.read_lockup(tx, seen)?;
        Ok(self.with(Stage::Locked(lockup)).settle(ctx))
    }

    fn read_lockup(&self, tx: &Transaction, seen: Seen) -> Result<LockupSeen, Refused> {
        let script = self.state.tree()?.script_pubkey();
        check_lockup(tx, &script, self.state.onchain_sats(), seen)
    }

    /// SW-40 to SW-44: build the claim of the lockup that was checked, at
    /// `fee_rate_per_kvb`. Returns the machine at "claim sent" and the
    /// transaction. The caller keeps the new machine before it hands the
    /// transaction to the network.
    pub fn claim(&self, fee_rate_per_kvb: u64, ctx: &Ctx) -> Result<(Self, Transaction), Refused> {
        let Stage::Locked(lockup) = self.stage else {
            return Err(Refused::NotReady("no lockup has been checked"));
        };
        let tx = build_claim(&self.state, &lockup, fee_rate_per_kvb, ctx.tip_height)?;
        let state = self.state.claimed(tx.compute_txid())?;
        Ok((
            Self {
                state,
                stage: Stage::ClaimSent,
            },
            tx,
        ))
    }

    /// SW-72: the caller saw the claim in a block of this height. Only the
    /// shop's own claim makes the swap paid.
    pub fn claim_confirmed(&self, txid: Txid, height: u32) -> Result<Self, Refused> {
        if self.stage != Stage::ClaimSent {
            return Err(Refused::NotReady("no claim is waiting"));
        }
        if self.state.phase() != Phase::Claimed(txid) {
            return Err(Refused::WrongClaim);
        }
        Ok(self.with(Stage::Paid { height }))
    }

    /// SW-73: the file.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(512);
        out.extend_from_slice(&MAGIC_FOLLOW);
        out.push(FOLLOW_VERSION);
        match self.stage {
            Stage::Created => out.push(0),
            Stage::Announced => out.push(1),
            Stage::Locked(l) => {
                out.push(2);
                out.extend_from_slice(l.outpoint.txid.as_ref());
                out.extend_from_slice(&l.outpoint.vout.to_be_bytes());
                out.extend_from_slice(&l.value_sats.to_be_bytes());
                out.push(u8::from(l.confirmed) | (u8::from(l.signals_rbf) << 1));
            }
            Stage::ClaimSent => out.push(3),
            Stage::Paid { height } => {
                out.push(4);
                out.extend_from_slice(&height.to_be_bytes());
            }
            Stage::Failed(f) => {
                out.push(5);
                out.push(f.to_byte());
            }
        }
        let inner = self.state.encode();
        out.extend_from_slice(&u16::try_from(inner.len()).unwrap_or(u16::MAX).to_be_bytes());
        out.extend_from_slice(&inner);
        out
    }

    /// SW-73: read a file, or refuse it. The stage must agree with the
    /// state's phase, a lockup must hold what was promised, and a file that
    /// decodes encodes back to the same bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, Refused> {
        let bad = Refused::BadFollow;
        let mut c = Reader(bytes);
        if c.array::<4>()? != MAGIC_FOLLOW {
            return Err(bad("magic"));
        }
        if c.array::<1>()?[0] != FOLLOW_VERSION {
            return Err(bad("version"));
        }
        let stage = match c.array::<1>()?[0] {
            0 => Stage::Created,
            1 => Stage::Announced,
            2 => {
                let txid = Txid::from_byte_array(c.array()?);
                let vout = u32::from_be_bytes(c.array()?);
                let value_sats = u64::from_be_bytes(c.array()?);
                let flags = c.array::<1>()?[0];
                if flags > 3 {
                    return Err(bad("flags"));
                }
                Stage::Locked(LockupSeen {
                    outpoint: OutPoint::new(txid, vout),
                    value_sats,
                    confirmed: flags & 1 != 0,
                    signals_rbf: flags & 2 != 0,
                })
            }
            3 => Stage::ClaimSent,
            4 => Stage::Paid {
                height: u32::from_be_bytes(c.array()?),
            },
            5 => Stage::Failed(Failure::from_byte(c.array::<1>()?[0]).ok_or(bad("failure"))?),
            _ => return Err(bad("stage")),
        };
        let len = usize::from(u16::from_be_bytes(c.array()?));
        let inner = c.take(len)?;
        if !c.0.is_empty() {
            return Err(bad("too long"));
        }
        let state = SwapState::decode(inner)?;
        let claimed = matches!(state.phase(), Phase::Claimed(_));
        let agrees = match stage {
            Stage::ClaimSent | Stage::Paid { .. } => claimed,
            Stage::Locked(l) => !claimed && l.value_sats >= state.onchain_sats(),
            _ => !claimed,
        };
        if !agrees {
            return Err(bad("stage and phase"));
        }
        let machine = Self { state, stage };
        if machine.encode() != bytes {
            return Err(bad("not canonical"));
        }
        Ok(machine)
    }
}

/// A reader of exact widths.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Refused> {
        let (head, rest) = self
            .0
            .split_at_checked(n)
            .ok_or(Refused::BadFollow("too short"))?;
        self.0 = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Refused> {
        <[u8; N]>::try_from(self.take(N)?).map_err(|_| Refused::BadFollow("too short"))
    }
}
