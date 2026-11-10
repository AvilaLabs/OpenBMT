//! Address batches for a counter.
//!
//! A counter never sees the wallet description. It holds batches: a
//! contiguous range of receive addresses, derived by Owner mode and signed
//! twice, once by the owner root and once by the Key app root. The counter
//! pinned both roots when it was paired and accepts a batch only with both
//! signatures. The Key app signs only after deriving the same range from its
//! own copy of the wallet and finding the same hash (§5.1).
//!
//! Hash and signatures cover the canonical bytes below, so a changed script,
//! range, counter or wallet gives a different hash and both signatures fail.
//!
//! ```text
//! wallet_checksum (32) | counter (32) | start u32 LE | count u32 LE
//!   | for each script: len u16 LE | script bytes
//! ```

use std::collections::{BTreeSet, HashSet, VecDeque};
use std::fmt;

use bitcoin::ScriptBuf;

use crate::identity::{tagged_hash, DeviceId, Identity, Sig, TAG_BATCH};
use crate::wallet::{WalletDescription, WalletError};
use crate::wire::{self, DecodeError, Reader};

/// Default smallest batch: about 8 weeks of sales, at least 1,000 addresses.
pub const DEFAULT_MIN_COUNT: u32 = 1_000;
/// Hard upper bound on one batch, to bound memory and message size.
pub const MAX_COUNT: u32 = 100_000;
/// Indexes at or above this are hardened and cannot be derived from an xpub.
const INDEX_LIMIT: u32 = 1 << 31;
/// Length of a P2WSH script, the only kind a batch holds.
const SCRIPT_LEN: usize = 34;

/// Rules for how big a batch must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchPolicy {
    /// Fewest addresses a batch may hold.
    pub min_count: u32,
}

impl Default for BatchPolicy {
    fn default() -> Self {
        Self {
            min_count: DEFAULT_MIN_COUNT,
        }
    }
}

/// Why a batch was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchError {
    /// Fewer addresses than the policy minimum.
    CountTooSmall { min: u32, got: u32 },
    /// More addresses than [`MAX_COUNT`].
    CountTooLarge,
    /// The range reaches hardened indexes or overflows.
    RangeOutOfBounds,
    /// `scripts.len()` is not `count`.
    ScriptCountMismatch,
    /// A script appears twice in the batch or was already issued.
    DuplicateScript,
    /// A script is not a P2WSH output.
    UnexpectedScriptType,
    /// The owner root did not sign this hash.
    OwnerSignatureInvalid,
    /// The Key app root did not sign this hash.
    ApproverSignatureInvalid,
    /// The batch is for another counter.
    WrongCounter,
    /// The batch is for another wallet.
    WrongWallet,
    /// The batch is for a counter that is not on the Key app's device list.
    UnknownCounter,
    /// The range overlaps one already issued, active or retired.
    OverlapsExisting,
    /// Owner and Key app roots must be different keys.
    SameRoot,
    /// The Key app derived different scripts than the batch holds.
    HashMismatch,
    /// The range starts above the high-water mark: addresses would be skipped.
    Gap { expected: u32, found: u32 },
    /// The range starts below the high-water mark: addresses would repeat.
    Overlap { expected: u32, found: u32 },
    /// Address derivation failed.
    Wallet(WalletError),
}

impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountTooSmall { min, got } => write!(f, "batch has {got} addresses, needs {min}"),
            Self::CountTooLarge => write!(f, "batch is too large"),
            Self::RangeOutOfBounds => write!(f, "address range out of bounds"),
            Self::ScriptCountMismatch => write!(f, "batch count does not match its scripts"),
            Self::DuplicateScript => write!(f, "batch repeats an address"),
            Self::UnexpectedScriptType => write!(f, "batch holds a non-P2WSH script"),
            Self::OwnerSignatureInvalid => write!(f, "owner signature missing or wrong"),
            Self::ApproverSignatureInvalid => write!(f, "Key app signature missing or wrong"),
            Self::WrongCounter => write!(f, "batch is for another counter"),
            Self::WrongWallet => write!(f, "batch is for another wallet"),
            Self::UnknownCounter => {
                write!(f, "batch is for a counter that is not on the device list")
            }
            Self::OverlapsExisting => write!(f, "batch overlaps addresses already issued"),
            Self::SameRoot => write!(f, "owner and Key app roots must differ"),
            Self::HashMismatch => write!(f, "addresses differ from the Key app's own derivation"),
            Self::Gap { expected, found } => {
                write!(
                    f,
                    "batch starts at {found}, expected {expected}: addresses skipped"
                )
            }
            Self::Overlap { expected, found } => {
                write!(
                    f,
                    "batch starts at {found}, expected {expected}: addresses repeated"
                )
            }
            Self::Wallet(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BatchError {}

impl From<WalletError> for BatchError {
    fn from(e: WalletError) -> Self {
        Self::Wallet(e)
    }
}

/// Why the Key app did not co-sign: the batch was refused, or the new
/// high-water mark could not be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CosignError<E> {
    /// The batch failed a check. Nothing was stored or signed.
    Refused(BatchError),
    /// The caller's store refused the new mark. Nothing was signed and the
    /// caller's mark is unchanged.
    Persist(E),
}

impl<E: fmt::Display> fmt::Display for CosignError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(e) => write!(f, "{e}"),
            Self::Persist(e) => write!(f, "could not store the new mark: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for CosignError<E> {}

impl<E> From<BatchError> for CosignError<E> {
    fn from(e: BatchError) -> Self {
        Self::Refused(e)
    }
}

/// Why a counter did not take a batch: it was refused, or the new state could
/// not be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddError<E> {
    /// The batch failed a check. The counter's state is unchanged.
    Refused(BatchError),
    /// The caller's store refused the new state. The counter's state is
    /// unchanged.
    Persist(E),
}

impl<E: fmt::Display> fmt::Display for AddError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(e) => write!(f, "{e}"),
            Self::Persist(e) => write!(f, "could not store the new state: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for AddError<E> {}

impl<E> From<BatchError> for AddError<E> {
    fn from(e: BatchError) -> Self {
        Self::Refused(e)
    }
}

/// The two roots a counter pinned at pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedRoots {
    /// Owner phone root.
    pub owner: DeviceId,
    /// Key app root.
    pub approver: DeviceId,
}

impl PinnedRoots {
    /// Pin two roots; they must be different keys.
    pub fn new(owner: DeviceId, approver: DeviceId) -> Result<Self, BatchError> {
        if owner == approver {
            return Err(BatchError::SameRoot);
        }
        Ok(Self { owner, approver })
    }
}

/// A contiguous range of receive addresses for one counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressBatch {
    /// SHA-256 of the wallet's canonical string.
    pub wallet_checksum: [u8; 32],
    /// The counter this batch is for.
    pub counter: DeviceId,
    /// First receive index.
    pub start: u32,
    /// Number of addresses.
    pub count: u32,
    /// The receive scripts for `start..start + count`, in order.
    pub scripts: Vec<ScriptBuf>,
}

impl AddressBatch {
    /// The canonical bytes that are hashed and signed.
    ///
    /// Every script must be a 34-byte P2WSH script, which `check_shape`
    /// enforces; every path that encodes a batch it did not build itself runs
    /// `check_shape` first. Calling this directly on a hand-built batch with
    /// a script longer than `u16::MAX` bytes panics rather than truncating
    /// the length.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(72 + self.scripts.len() * 36);
        out.extend_from_slice(&self.wallet_checksum);
        out.extend_from_slice(&self.counter.to_bytes());
        out.extend_from_slice(&self.start.to_le_bytes());
        out.extend_from_slice(&self.count.to_le_bytes());
        for s in &self.scripts {
            let b = s.as_bytes();
            let len = u16::try_from(b.len()).expect("script length checked by check_shape");
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(b);
        }
        out
    }

    /// Tagged hash (`OpenBMT/batch/v1`) of the canonical bytes. This is what
    /// both roots sign.
    pub fn hash(&self) -> [u8; 32] {
        tagged_hash(TAG_BATCH, &self.encode())
    }

    /// One past the last index, or `None` if the range is out of bounds.
    pub fn end(&self) -> Option<u32> {
        let end = self.start.checked_add(self.count)?;
        (end <= INDEX_LIMIT).then_some(end)
    }

    /// Sign this batch with `identity` (owner or Key app).
    pub fn sign(&self, identity: &Identity) -> Sig {
        identity.sign_digest(&self.hash())
    }

    /// Read the bytes written by [`AddressBatch::encode`]. This only parses:
    /// the shape check and the signatures are still the caller's to run.
    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let wallet_checksum = r.array::<32>()?;
        let counter = r.device_id()?;
        let start = r.u32_le()?;
        let n = r.count(MAX_COUNT as usize, 2 + SCRIPT_LEN)?;
        let count = u32::try_from(n).map_err(|_| DecodeError::TooLarge)?;
        let mut scripts = Vec::with_capacity(n);
        for _ in 0..n {
            if usize::from(r.u16_le()?) != SCRIPT_LEN {
                return Err(DecodeError::NonCanonical("script is not 34 bytes"));
            }
            scripts.push(ScriptBuf::from_bytes(r.take(SCRIPT_LEN)?.to_vec()));
        }
        Ok(Self {
            wallet_checksum,
            counter,
            start,
            count,
            scripts,
        })
    }

    fn check_shape(&self, policy: &BatchPolicy) -> Result<(), BatchError> {
        if self.count < policy.min_count {
            return Err(BatchError::CountTooSmall {
                min: policy.min_count,
                got: self.count,
            });
        }
        if self.count > MAX_COUNT {
            return Err(BatchError::CountTooLarge);
        }
        self.end().ok_or(BatchError::RangeOutOfBounds)?;
        if self.scripts.len() != self.count as usize {
            return Err(BatchError::ScriptCountMismatch);
        }
        let mut seen = HashSet::with_capacity(self.scripts.len());
        for s in &self.scripts {
            if !s.is_p2wsh() {
                return Err(BatchError::UnexpectedScriptType);
            }
            if !seen.insert(s) {
                return Err(BatchError::DuplicateScript);
            }
        }
        Ok(())
    }
}

/// Derives batches from a wallet.
#[derive(Debug, Clone, Copy, Default)]
pub struct BatchBuilder {
    policy: BatchPolicy,
}

impl BatchBuilder {
    /// A builder that enforces `policy`.
    pub fn new(policy: BatchPolicy) -> Self {
        Self { policy }
    }

    /// Derive `count` receive scripts starting at `start` for `counter`.
    pub fn derive(
        &self,
        wallet: &WalletDescription,
        counter: DeviceId,
        start: u32,
        count: u32,
    ) -> Result<AddressBatch, BatchError> {
        if count < self.policy.min_count {
            return Err(BatchError::CountTooSmall {
                min: self.policy.min_count,
                got: count,
            });
        }
        if count > MAX_COUNT {
            return Err(BatchError::CountTooLarge);
        }
        let end = start
            .checked_add(count)
            .ok_or(BatchError::RangeOutOfBounds)?;
        if end > INDEX_LIMIT {
            return Err(BatchError::RangeOutOfBounds);
        }
        let scripts = (start..end)
            .map(|i| wallet.receive_script(i))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AddressBatch {
            wallet_checksum: wallet.checksum32(),
            counter,
            start,
            count,
            scripts,
        })
    }
}

/// A batch with both signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedBatch {
    /// The batch.
    pub batch: AddressBatch,
    /// Owner root signature over `batch.hash()`.
    pub owner_sig: Sig,
    /// Key app root signature over `batch.hash()`.
    pub approver_sig: Sig,
}

impl SignedBatch {
    /// Check shape and both signatures against the pinned roots. One
    /// signature, the wrong signer, or changed scripts all fail.
    pub fn verify(&self, roots: &PinnedRoots, policy: &BatchPolicy) -> Result<(), BatchError> {
        self.batch.check_shape(policy)?;
        let h = self.batch.hash();
        if !roots.owner.verify_digest(&h, &self.owner_sig) {
            return Err(BatchError::OwnerSignatureInvalid);
        }
        if !roots.approver.verify_digest(&h, &self.approver_sig) {
            return Err(BatchError::ApproverSignatureInvalid);
        }
        Ok(())
    }

    /// Wire form: `OBMB`, version 1, the bytes of [`AddressBatch::encode`]
    /// (the bytes that are hashed and signed), then the owner and Key app
    /// signatures. Call it on a batch that passed `verify` or was built by
    /// the derivation code; `encode` panics on a script longer than
    /// `u16::MAX` bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = self.batch.encode();
        let mut out = wire::start(&wire::MAGIC_SIGNED_BATCH, body.len() + 128);
        out.extend_from_slice(&body);
        out.extend_from_slice(&self.owner_sig.0);
        out.extend_from_slice(&self.approver_sig.0);
        out
    }

    /// Parse the wire form strictly (see [`crate::wire`]). This does not
    /// check shape or signatures: call [`SignedBatch::verify`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_SIGNED_BATCH)?;
        let batch = AddressBatch::read(&mut r)?;
        let owner_sig = r.sig()?;
        let approver_sig = r.sig()?;
        r.finish()?;
        Ok(Self {
            batch,
            owner_sig,
            approver_sig,
        })
    }
}

/// The Key app's monotonic mark: the first index it has not yet signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HighWater(u32);

impl HighWater {
    /// A fresh mark: nothing signed yet.
    pub fn new() -> Self {
        Self(0)
    }

    /// Restore a stored mark.
    pub fn from_next_index(next: u32) -> Self {
        Self(next)
    }

    /// The only `start` the next batch may have.
    pub fn next_index(&self) -> u32 {
        self.0
    }

    /// Snapshot: `OBMH`, version 1, the next index as u32 LE.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&wire::MAGIC_HIGH_WATER, 4);
        out.extend_from_slice(&self.0.to_le_bytes());
        out
    }

    /// Restore a snapshot strictly (see [`crate::wire`]).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_HIGH_WATER)?;
        let next = r.u32_le()?;
        r.finish()?;
        Ok(Self(next))
    }
}

/// The Key app's batch check (§5.1), as a pure function over its own state.
pub struct Approver {
    identity: Identity,
    builder: BatchBuilder,
}

impl Approver {
    /// An approver that signs with `identity` and enforces `policy`.
    pub fn new(identity: Identity, policy: BatchPolicy) -> Self {
        Self {
            identity,
            builder: BatchBuilder::new(policy),
        }
    }

    /// This approver's public root.
    pub fn device_id(&self) -> DeviceId {
        self.identity.device_id()
    }

    /// Check the batch is for a counter in `approved_counters` (the Key app's
    /// device list), re-derive its range from `own_wallet`, compare the hash,
    /// check the range starts exactly at the high-water mark (no gap, no
    /// overlap), then co-sign and advance the mark.
    ///
    /// The new mark is computed on a copy and handed to `persist`. Only if
    /// `persist` returns `Ok` is the caller's `high_water` updated and the
    /// signature returned; on a refusal or a persist error nothing is signed
    /// and the mark does not move. The order matters: if the signature could
    /// exist before the mark is stored, a crash in between would lose the
    /// mark and the same index range could be signed again for a second
    /// counter.
    pub fn check_and_cosign<E>(
        &self,
        own_wallet: &WalletDescription,
        batch: &AddressBatch,
        high_water: &mut HighWater,
        approved_counters: &BTreeSet<DeviceId>,
        persist: impl FnOnce(&HighWater) -> Result<(), E>,
    ) -> Result<Sig, CosignError<E>> {
        if batch.wallet_checksum != own_wallet.checksum32() {
            return Err(BatchError::WrongWallet.into());
        }
        if !approved_counters.contains(&batch.counter) {
            return Err(BatchError::UnknownCounter.into());
        }
        let end = batch.end().ok_or(BatchError::RangeOutOfBounds)?;
        match batch.start.cmp(&high_water.0) {
            std::cmp::Ordering::Greater => {
                return Err(BatchError::Gap {
                    expected: high_water.0,
                    found: batch.start,
                }
                .into())
            }
            std::cmp::Ordering::Less => {
                return Err(BatchError::Overlap {
                    expected: high_water.0,
                    found: batch.start,
                }
                .into())
            }
            std::cmp::Ordering::Equal => {}
        }
        // `batch` is untrusted: its scripts must pass the shape check before
        // they are encoded and hashed, and before any derivation work is done.
        batch.check_shape(&self.builder.policy)?;
        let own = self
            .builder
            .derive(own_wallet, batch.counter, batch.start, batch.count)?;
        if own.hash() != batch.hash() {
            return Err(BatchError::HashMismatch.into());
        }
        let new_mark = HighWater(end);
        persist(&new_mark).map_err(CosignError::Persist)?;
        *high_water = new_mark;
        Ok(self.identity.sign_digest(&own.hash()))
    }
}

#[derive(Debug, Clone)]
struct ActiveBatch {
    start: u32,
    scripts: Vec<ScriptBuf>,
    next: usize,
}

/// The counter's verified batches and its `next_address` state.
#[derive(Debug, Clone)]
pub struct CounterBatches {
    counter: DeviceId,
    roots: PinnedRoots,
    wallet_checksum: [u8; 32],
    policy: BatchPolicy,
    active: VecDeque<ActiveBatch>,
    ranges: Vec<(u32, u32)>,
    known: HashSet<ScriptBuf>,
    watched: Vec<ScriptBuf>,
}

impl CounterBatches {
    /// State for `counter`, with the roots and wallet checksum pinned at
    /// pairing.
    pub fn new(
        counter: DeviceId,
        roots: PinnedRoots,
        wallet_checksum: [u8; 32],
        policy: BatchPolicy,
    ) -> Self {
        Self {
            counter,
            roots,
            wallet_checksum,
            policy,
            active: VecDeque::new(),
            ranges: Vec::new(),
            known: HashSet::new(),
            watched: Vec::new(),
        }
    }

    /// Verify and store a batch.
    ///
    /// The state as it will be after the batch is added is serialized
    /// ([`Self::to_bytes`]) and handed to `persist`; the batch is stored only
    /// if `persist` returns `Ok`, and on any error the state is unchanged.
    /// Storing the range before it is saved would let a crash forget that the
    /// range was taken, and a replayed batch could then be accepted again.
    pub fn add<E>(
        &mut self,
        signed: SignedBatch,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), AddError<E>> {
        if signed.batch.counter != self.counter {
            return Err(BatchError::WrongCounter.into());
        }
        if signed.batch.wallet_checksum != self.wallet_checksum {
            return Err(BatchError::WrongWallet.into());
        }
        signed.verify(&self.roots, &self.policy)?;
        let start = signed.batch.start;
        let end = signed.batch.end().ok_or(BatchError::RangeOutOfBounds)?;
        if self.ranges.iter().any(|&(s, e)| start < e && s < end) {
            return Err(BatchError::OverlapsExisting.into());
        }
        if signed.batch.scripts.iter().any(|s| self.known.contains(s)) {
            return Err(BatchError::DuplicateScript.into());
        }
        let mut next = self.clone();
        next.known.extend(signed.batch.scripts.iter().cloned());
        next.ranges.push((start, end));
        let pos = next.active.partition_point(|b| b.start < start);
        next.active.insert(
            pos,
            ActiveBatch {
                start,
                scripts: signed.batch.scripts,
                next: 0,
            },
        );
        persist(&next.to_bytes()).map_err(AddError::Persist)?;
        *self = next;
        Ok(())
    }

    /// The next unused script and its index. Each script is returned once.
    /// `None` means the range is empty: the caller must not offer Bitcoin.
    ///
    /// The state as it will be once the script is handed out is serialized
    /// ([`Self::to_bytes`]) and given to `persist`. The script is released
    /// only if `persist` returns `Ok`; on an error the state is unchanged and
    /// nothing is returned. The order matters: if an address could be shown
    /// before the fact is stored, a crash would bring the same address back
    /// and two customers could pay it. When nothing is left, `persist` is not
    /// called. If an index would pass the derivable range (which a verified
    /// batch never does), no script is returned.
    ///
    /// Each call works on a copy of the state, so its cost grows with the
    /// number of stored scripts, the same as the snapshot it writes.
    pub fn next_script<E>(
        &mut self,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Option<(u32, ScriptBuf)>, E> {
        let mut next = self.clone();
        let Some(issued) = next.take_next() else {
            return Ok(None);
        };
        persist(&next.to_bytes())?;
        *self = next;
        Ok(Some(issued))
    }

    fn take_next(&mut self) -> Option<(u32, ScriptBuf)> {
        loop {
            let front = self.active.front_mut()?;
            if front.next < front.scripts.len() {
                let offset = u32::try_from(front.next).ok()?;
                let i = front.start.checked_add(offset)?;
                let s = front.scripts[front.next].clone();
                front.next += 1;
                self.watched.push(s.clone());
                return Some((i, s));
            }
            self.active.pop_front();
        }
    }

    /// How many unused scripts are left.
    pub fn remaining(&self) -> usize {
        self.active.iter().map(|b| b.scripts.len() - b.next).sum()
    }

    /// Retire every unused script (after a reset or restore). They are
    /// still watched, but never handed out, and their index range stays
    /// blocked so a fresh batch must cover new indexes. Returns how many
    /// were retired.
    ///
    /// The resulting state is serialized ([`Self::to_bytes`]) and given to
    /// `persist`; scripts are retired only if it returns `Ok`, and on an
    /// error the state is unchanged. If the retirement were lost in a crash,
    /// scripts that a reset meant to withdraw would be handed out again. When
    /// no batch is active, `persist` is not called.
    pub fn retire_unused<E>(
        &mut self,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<usize, E> {
        if self.active.is_empty() {
            return Ok(0);
        }
        let mut next = self.clone();
        let mut n = 0;
        for b in next.active.drain(..) {
            n += b.scripts.len() - b.next;
            next.watched.extend(b.scripts.into_iter().skip(b.next));
        }
        persist(&next.to_bytes())?;
        *self = next;
        Ok(n)
    }

    /// Every script that was shown or retired. These stay watched forever.
    pub fn watched_scripts(&self) -> &[ScriptBuf] {
        &self.watched
    }

    /// Snapshot of everything needed to resume exactly: the issued index
    /// ranges (retired ones included), each active batch with its scripts and
    /// the position of the next unused one, and the watched scripts in the
    /// order they were shown or retired. The pinned roots, the policy, and
    /// the counter and wallet the state belongs to are not trusted from
    /// storage: the counter and wallet checksum are written so a snapshot
    /// for another counter or wallet is noticed, and [`Self::from_bytes`]
    /// takes all four from the caller.
    ///
    /// ```text
    /// OBMC | 1 | counter (32) | wallet checksum (32)
    ///   | n_ranges u32 | (start u32, end u32)*
    ///   | n_active u32 | (start u32, next u32, n u32, n * 34-byte script)*
    ///   | n_watched u32 | n_watched * 34-byte script
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        let scripts: usize = self.active.iter().map(|b| b.scripts.len()).sum();
        let mut out = wire::start(
            &wire::MAGIC_COUNTER_BATCHES,
            76 + self.ranges.len() * 8
                + self.active.len() * 12
                + (scripts + self.watched.len()) * SCRIPT_LEN,
        );
        out.extend_from_slice(&self.counter.to_bytes());
        out.extend_from_slice(&self.wallet_checksum);
        out.extend_from_slice(&wire::len_u32(self.ranges.len()).to_le_bytes());
        for &(start, end) in &self.ranges {
            out.extend_from_slice(&start.to_le_bytes());
            out.extend_from_slice(&end.to_le_bytes());
        }
        out.extend_from_slice(&wire::len_u32(self.active.len()).to_le_bytes());
        for b in &self.active {
            out.extend_from_slice(&b.start.to_le_bytes());
            out.extend_from_slice(&wire::len_u32(b.next).to_le_bytes());
            out.extend_from_slice(&wire::len_u32(b.scripts.len()).to_le_bytes());
            for s in &b.scripts {
                out.extend_from_slice(s.as_bytes());
            }
        }
        out.extend_from_slice(&wire::len_u32(self.watched.len()).to_le_bytes());
        for s in &self.watched {
            out.extend_from_slice(s.as_bytes());
        }
        out
    }

    /// Restore a snapshot strictly (see [`crate::wire`]) for `counter`.
    ///
    /// `roots`, `wallet_checksum` and `policy` come from the caller, as they
    /// do for [`CounterBatches::new`], so stored bytes cannot change who is
    /// trusted. The snapshot must be for the same counter and wallet, and the
    /// invariants the type keeps must hold, else the result is
    /// `NonCanonical`: issued ranges do not overlap, each active batch fills
    /// exactly one issued range in ascending order with its position inside
    /// it, every script is a P2WSH script, no script appears twice, and the
    /// scripts already shown from an active batch are exactly the watched
    /// ones. The result then behaves exactly as the snapshotted state did.
    pub fn from_bytes(
        bytes: &[u8],
        counter: DeviceId,
        roots: PinnedRoots,
        wallet_checksum: [u8; 32],
        policy: BatchPolicy,
    ) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_COUNTER_BATCHES)?;
        if r.device_id()? != counter {
            return Err(DecodeError::NonCanonical("snapshot is for another counter"));
        }
        if r.array::<32>()? != wallet_checksum {
            return Err(DecodeError::NonCanonical("snapshot is for another wallet"));
        }

        let n = r.count(usize::MAX, 8)?;
        let mut ranges = Vec::with_capacity(n);
        for _ in 0..n {
            let (start, end) = (r.u32_le()?, r.u32_le()?);
            if start >= end || end > INDEX_LIMIT {
                return Err(DecodeError::NonCanonical(
                    "issued range is empty or out of bounds",
                ));
            }
            ranges.push((start, end));
        }
        let mut sorted = ranges.clone();
        sorted.sort_unstable();
        if sorted.windows(2).any(|w| w[1].0 < w[0].1) {
            return Err(DecodeError::NonCanonical("issued ranges overlap"));
        }

        let n = r.count(usize::MAX, 12)?;
        let mut active: VecDeque<ActiveBatch> = VecDeque::with_capacity(n);
        for _ in 0..n {
            let start = r.u32_le()?;
            let next = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
            let len = r.count(MAX_COUNT as usize, SCRIPT_LEN)?;
            if next > len {
                return Err(DecodeError::NonCanonical("position is past the batch"));
            }
            let end = u32::try_from(len)
                .ok()
                .and_then(|l| start.checked_add(l))
                .filter(|&e| e <= INDEX_LIMIT)
                .ok_or(DecodeError::NonCanonical("active batch is out of bounds"))?;
            if sorted.binary_search(&(start, end)).is_err() {
                return Err(DecodeError::NonCanonical(
                    "active batch is not an issued range",
                ));
            }
            if active.back().is_some_and(|prev| prev.start >= start) {
                return Err(DecodeError::NonCanonical(
                    "active batches are not in ascending order",
                ));
            }
            let mut scripts = Vec::with_capacity(len);
            for _ in 0..len {
                scripts.push(read_p2wsh(&mut r)?);
            }
            active.push_back(ActiveBatch {
                start,
                scripts,
                next,
            });
        }

        let n = r.count(usize::MAX, SCRIPT_LEN)?;
        let mut watched = Vec::with_capacity(n);
        for _ in 0..n {
            watched.push(read_p2wsh(&mut r)?);
        }
        r.finish()?;

        let mut watched_set = HashSet::with_capacity(watched.len());
        if !watched.iter().all(|s| watched_set.insert(s)) {
            return Err(DecodeError::NonCanonical("watched scripts repeat"));
        }
        let mut known: HashSet<ScriptBuf> = watched.iter().cloned().collect();
        let mut active_seen = HashSet::new();
        for b in &active {
            for (i, s) in b.scripts.iter().enumerate() {
                if !active_seen.insert(s) {
                    return Err(DecodeError::NonCanonical("an active script repeats"));
                }
                let shown = i < b.next;
                if watched_set.contains(s) != shown {
                    return Err(DecodeError::NonCanonical(
                        "shown scripts do not match the watched scripts",
                    ));
                }
                if !shown {
                    known.insert(s.clone());
                }
            }
        }
        let issued: u64 = ranges.iter().map(|&(s, e)| u64::from(e - s)).sum();
        if watched.len() as u64 > issued {
            return Err(DecodeError::NonCanonical(
                "more watched scripts than issued addresses",
            ));
        }

        Ok(Self {
            counter,
            roots,
            wallet_checksum,
            policy,
            active,
            ranges,
            known,
            watched,
        })
    }
}

/// Read one 34-byte script, which must be P2WSH.
fn read_p2wsh(r: &mut Reader<'_>) -> Result<ScriptBuf, DecodeError> {
    let script = ScriptBuf::from_bytes(r.take(SCRIPT_LEN)?.to_vec());
    if !script.is_p2wsh() {
        return Err(DecodeError::NonCanonical("script is not P2WSH"));
    }
    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{identity, persist_fail, persist_ok, wallet};
    use bitcoin::Network;
    use std::convert::Infallible;

    const SMALL: BatchPolicy = BatchPolicy { min_count: 3 };

    /// The Key app check with a store that always succeeds, refusals flattened
    /// to `BatchError`.
    trait Cosign {
        fn cosign(
            &self,
            wallet: &WalletDescription,
            batch: &AddressBatch,
            hw: &mut HighWater,
            approved: &BTreeSet<DeviceId>,
        ) -> Result<Sig, BatchError>;
    }

    impl Cosign for Approver {
        fn cosign(
            &self,
            wallet: &WalletDescription,
            batch: &AddressBatch,
            hw: &mut HighWater,
            approved: &BTreeSet<DeviceId>,
        ) -> Result<Sig, BatchError> {
            match self.check_and_cosign(wallet, batch, hw, approved, persist_ok) {
                Ok(sig) => Ok(sig),
                Err(CosignError::Refused(e)) => Err(e),
                Err(CosignError::Persist(never)) => match never {},
            }
        }
    }

    /// The counter's mutating calls with a store that always succeeds.
    trait Quick {
        fn add_ok(&mut self, signed: SignedBatch) -> Result<(), BatchError>;
        fn next_ok(&mut self) -> Option<(u32, ScriptBuf)>;
        fn retire_ok(&mut self) -> usize;
    }

    impl Quick for CounterBatches {
        fn add_ok(&mut self, signed: SignedBatch) -> Result<(), BatchError> {
            match self.add(signed, persist_ok) {
                Ok(()) => Ok(()),
                Err(AddError::Refused(e)) => Err(e),
                Err(AddError::Persist(never)) => match never {},
            }
        }

        fn next_ok(&mut self) -> Option<(u32, ScriptBuf)> {
            match self.next_script(persist_ok) {
                Ok(v) => v,
                Err(never) => match never {},
            }
        }

        fn retire_ok(&mut self) -> usize {
            match self.retire_unused(persist_ok) {
                Ok(n) => n,
                Err(never) => match never {},
            }
        }
    }

    struct Fx {
        wallet: WalletDescription,
        owner: Identity,
        key_app: Approver,
        counter: DeviceId,
        roots: PinnedRoots,
        approved: BTreeSet<DeviceId>,
    }

    fn fx() -> Fx {
        let owner = identity(10);
        let key_app = Approver::new(identity(11), SMALL);
        let counter = identity(12).device_id();
        let roots = PinnedRoots::new(owner.device_id(), key_app.device_id()).unwrap();
        Fx {
            wallet: wallet(Network::Testnet),
            owner,
            key_app,
            counter,
            roots,
            approved: BTreeSet::from([counter]),
        }
    }

    fn signed(f: &Fx, start: u32, count: u32, hw: &mut HighWater) -> SignedBatch {
        let batch = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, start, count)
            .unwrap();
        let approver_sig = f
            .key_app
            .cosign(&f.wallet, &batch, hw, &f.approved)
            .unwrap();
        SignedBatch {
            owner_sig: batch.sign(&f.owner),
            approver_sig,
            batch,
        }
    }

    fn counter_state(f: &Fx) -> CounterBatches {
        CounterBatches::new(f.counter, f.roots, f.wallet.checksum32(), SMALL)
    }

    #[test]
    fn default_policy_minimum_is_1000() {
        assert_eq!(BatchPolicy::default().min_count, 1_000);
        let f = fx();
        let b = BatchBuilder::default();
        assert_eq!(
            b.derive(&f.wallet, f.counter, 0, 999).unwrap_err(),
            BatchError::CountTooSmall {
                min: 1000,
                got: 999
            }
        );
    }

    #[test]
    fn default_policy_thousand_address_batch_end_to_end() {
        let f = fx();
        let approver = Approver::new(identity(11), BatchPolicy::default());
        let batch = BatchBuilder::default()
            .derive(&f.wallet, f.counter, 0, 1_000)
            .unwrap();
        assert_eq!(batch.scripts.len(), 1_000);
        let mut hw = HighWater::new();
        let sig = approver
            .cosign(&f.wallet, &batch, &mut hw, &f.approved)
            .unwrap();
        assert_eq!(hw.next_index(), 1_000);
        let sb = SignedBatch {
            owner_sig: batch.sign(&f.owner),
            approver_sig: sig,
            batch,
        };
        sb.verify(&f.roots, &BatchPolicy::default()).unwrap();
    }

    #[test]
    fn builder_matches_wallet_scripts() {
        let f = fx();
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 7, 5)
            .unwrap();
        assert_eq!(b.start, 7);
        assert_eq!(b.count, 5);
        for (k, s) in b.scripts.iter().enumerate() {
            assert_eq!(*s, f.wallet.receive_script(7 + k as u32).unwrap());
        }
        assert_eq!(b.wallet_checksum, f.wallet.checksum32());
    }

    #[test]
    fn builder_rejects_bad_ranges() {
        let f = fx();
        let b = BatchBuilder::new(SMALL);
        assert!(matches!(
            b.derive(&f.wallet, f.counter, 0, 2),
            Err(BatchError::CountTooSmall { .. })
        ));
        assert_eq!(
            b.derive(&f.wallet, f.counter, 0, MAX_COUNT + 1)
                .unwrap_err(),
            BatchError::CountTooLarge
        );
        assert_eq!(
            b.derive(&f.wallet, f.counter, u32::MAX - 1, 5).unwrap_err(),
            BatchError::RangeOutOfBounds
        );
        assert_eq!(
            b.derive(&f.wallet, f.counter, (1 << 31) - 2, 5)
                .unwrap_err(),
            BatchError::RangeOutOfBounds
        );
    }

    #[test]
    fn hash_covers_every_field() {
        let f = fx();
        let base = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        let h = base.hash();
        assert_eq!(h, base.clone().hash());
        let mut m = base.clone();
        m.counter = identity(99).device_id();
        assert_ne!(m.hash(), h);
        let mut m = base.clone();
        m.wallet_checksum[0] ^= 1;
        assert_ne!(m.hash(), h);
        let mut m = base.clone();
        m.start += 1;
        assert_ne!(m.hash(), h);
        let mut m = base.clone();
        m.scripts.swap(0, 1);
        assert_ne!(m.hash(), h);
        let mut m = base;
        m.count += 1;
        assert_ne!(m.hash(), h);
    }

    #[test]
    fn two_valid_signatures_verify() {
        let f = fx();
        let sb = signed(&f, 0, 3, &mut HighWater::new());
        sb.verify(&f.roots, &SMALL).unwrap();
    }

    #[test]
    fn one_signature_is_rejected() {
        let f = fx();
        let mut sb = signed(&f, 0, 3, &mut HighWater::new());
        sb.approver_sig = Sig([0; 64]);
        assert_eq!(
            sb.verify(&f.roots, &SMALL),
            Err(BatchError::ApproverSignatureInvalid)
        );
        let mut sb = signed(&f, 0, 3, &mut HighWater::new());
        sb.owner_sig = Sig([0; 64]);
        assert_eq!(
            sb.verify(&f.roots, &SMALL),
            Err(BatchError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn owner_signing_twice_is_rejected() {
        let f = fx();
        let mut sb = signed(&f, 0, 3, &mut HighWater::new());
        sb.approver_sig = sb.batch.sign(&f.owner);
        assert_eq!(
            sb.verify(&f.roots, &SMALL),
            Err(BatchError::ApproverSignatureInvalid)
        );
    }

    #[test]
    fn swapped_or_unpinned_signers_are_rejected() {
        let f = fx();
        let sb = signed(&f, 0, 3, &mut HighWater::new());
        let swapped = SignedBatch {
            owner_sig: sb.approver_sig,
            approver_sig: sb.owner_sig,
            batch: sb.batch.clone(),
        };
        assert_eq!(
            swapped.verify(&f.roots, &SMALL),
            Err(BatchError::OwnerSignatureInvalid)
        );
        let rogue = identity(77);
        let forged = SignedBatch {
            owner_sig: sb.batch.sign(&rogue),
            approver_sig: sb.approver_sig,
            batch: sb.batch,
        };
        assert_eq!(
            forged.verify(&f.roots, &SMALL),
            Err(BatchError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn tampered_scripts_are_rejected() {
        let f = fx();
        let mut sb = signed(&f, 0, 3, &mut HighWater::new());
        // an attacker's address, same type
        sb.batch.scripts[1] = f.wallet.receive_script(500).unwrap();
        assert_eq!(
            sb.verify(&f.roots, &SMALL),
            Err(BatchError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn shape_errors_are_rejected_even_when_signed() {
        let f = fx();
        let make = |edit: &dyn Fn(&mut AddressBatch)| {
            let mut sb = signed(&f, 0, 3, &mut HighWater::new());
            edit(&mut sb.batch);
            sb.owner_sig = sb.batch.sign(&f.owner);
            sb.approver_sig = sb.batch.sign(&f.key_app.identity);
            sb.verify(&f.roots, &SMALL)
        };
        assert_eq!(make(&|b| b.count = 4), Err(BatchError::ScriptCountMismatch));
        assert_eq!(
            make(&|b| b.scripts[2] = b.scripts[0].clone()),
            Err(BatchError::DuplicateScript)
        );
        assert_eq!(
            make(&|b| b.scripts[0] = ScriptBuf::new()),
            Err(BatchError::UnexpectedScriptType)
        );
        assert!(matches!(
            make(&|b| {
                b.scripts.truncate(2);
                b.count = 2
            }),
            Err(BatchError::CountTooSmall { .. })
        ));
        assert_eq!(
            make(&|b| b.start = u32::MAX),
            Err(BatchError::RangeOutOfBounds)
        );
    }

    #[test]
    fn policy_minimum_applies_when_verifying() {
        let f = fx();
        let sb = signed(&f, 0, 3, &mut HighWater::new());
        assert!(matches!(
            sb.verify(&f.roots, &BatchPolicy::default()),
            Err(BatchError::CountTooSmall { min: 1000, got: 3 })
        ));
    }

    #[test]
    fn pinned_roots_must_differ() {
        let a = identity(1).device_id();
        assert_eq!(PinnedRoots::new(a, a).unwrap_err(), BatchError::SameRoot);
    }

    #[test]
    fn approver_advances_high_water_contiguously() {
        let f = fx();
        let mut hw = HighWater::new();
        signed(&f, 0, 3, &mut hw);
        assert_eq!(hw.next_index(), 3);
        signed(&f, 3, 4, &mut hw);
        assert_eq!(hw.next_index(), 7);
    }

    #[test]
    fn approver_rejects_gap_and_overlap_without_moving_the_mark() {
        let f = fx();
        let mut hw = HighWater::from_next_index(10);
        let mk = |start| {
            BatchBuilder::new(SMALL)
                .derive(&f.wallet, f.counter, start, 3)
                .unwrap()
        };
        assert_eq!(
            f.key_app
                .cosign(&f.wallet, &mk(11), &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::Gap {
                expected: 10,
                found: 11
            }
        );
        assert_eq!(
            f.key_app
                .cosign(&f.wallet, &mk(9), &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::Overlap {
                expected: 10,
                found: 9
            }
        );
        assert_eq!(
            f.key_app
                .cosign(&f.wallet, &mk(0), &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::Overlap {
                expected: 10,
                found: 0
            }
        );
        assert_eq!(hw.next_index(), 10);
        // the same range cannot be signed twice
        f.key_app
            .cosign(&f.wallet, &mk(10), &mut hw, &f.approved)
            .unwrap();
        assert!(f
            .key_app
            .cosign(&f.wallet, &mk(10), &mut hw, &f.approved)
            .is_err());
        assert_eq!(hw.next_index(), 13);
    }

    #[test]
    fn approver_rejects_attacker_scripts() {
        let f = fx();
        let mut hw = HighWater::new();
        let mut b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        b.scripts[1] = f.wallet.change_script(1).unwrap();
        assert_eq!(
            f.key_app
                .cosign(&f.wallet, &b, &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::HashMismatch
        );
        assert_eq!(hw.next_index(), 0);
    }

    #[test]
    fn approver_rejects_other_wallet_and_hardened_range() {
        let f = fx();
        let mut hw = HighWater::new();
        let other = crate::wallet::WalletDescription::new(
            Network::Testnet,
            [
                crate::testutil::slot_from_mnemonic(
                    crate::testutil::MNEMONICS[1],
                    1,
                    Network::Testnet,
                ),
                crate::testutil::slot_from_mnemonic(
                    crate::testutil::MNEMONICS[2],
                    2,
                    Network::Testnet,
                ),
                crate::testutil::slot_from_mnemonic(
                    crate::testutil::MNEMONICS[3],
                    3,
                    Network::Testnet,
                ),
            ],
        )
        .unwrap();
        let b = BatchBuilder::new(SMALL)
            .derive(&other, f.counter, 0, 3)
            .unwrap();
        assert_eq!(
            f.key_app
                .cosign(&f.wallet, &b, &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::WrongWallet
        );
        let mut b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        b.start = u32::MAX;
        assert_eq!(
            f.key_app
                .cosign(&f.wallet, &b, &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::RangeOutOfBounds
        );
    }

    #[test]
    fn approver_refuses_an_unknown_counter_and_keeps_the_mark() {
        let f = fx();
        let stranger = identity(99).device_id();
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, stranger, 0, 3)
            .unwrap();
        let mut hw = HighWater::from_next_index(0);
        assert_eq!(
            f.key_app
                .cosign(&f.wallet, &b, &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::UnknownCounter
        );
        assert_eq!(hw.next_index(), 0);
        // An empty device list refuses everything.
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        assert_eq!(
            f.key_app
                .cosign(&f.wallet, &b, &mut hw, &BTreeSet::new())
                .unwrap_err(),
            BatchError::UnknownCounter
        );
        assert_eq!(hw.next_index(), 0);
    }

    #[test]
    fn approver_signs_for_a_known_counter() {
        let f = fx();
        let mut approved = f.approved.clone();
        approved.insert(identity(99).device_id());
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        let mut hw = HighWater::new();
        f.key_app.cosign(&f.wallet, &b, &mut hw, &approved).unwrap();
        assert_eq!(hw.next_index(), 3);
    }

    #[test]
    fn approver_runs_the_shape_check_before_hashing() {
        let f = fx();
        let mut b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        // An oversized script would otherwise reach `encode`.
        b.scripts[1] = ScriptBuf::from_bytes(vec![0x51; 70_000]);
        let mut hw = HighWater::new();
        assert_eq!(
            f.key_app
                .cosign(&f.wallet, &b, &mut hw, &f.approved)
                .unwrap_err(),
            BatchError::UnexpectedScriptType
        );
        assert_eq!(hw.next_index(), 0);
    }

    #[test]
    fn approver_enforces_its_minimum() {
        let f = fx();
        let strict = Approver::new(identity(11), BatchPolicy::default());
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        assert!(matches!(
            strict.cosign(&f.wallet, &b, &mut HighWater::new(), &f.approved),
            Err(BatchError::CountTooSmall { .. })
        ));
    }

    #[test]
    fn counter_issues_each_script_once_in_order() {
        let f = fx();
        let mut c = counter_state(&f);
        assert_eq!(c.next_ok(), None);
        c.add_ok(signed(&f, 0, 3, &mut HighWater::new())).unwrap();
        assert_eq!(c.remaining(), 3);
        let mut seen = HashSet::new();
        for i in 0..3 {
            let (idx, s) = c.next_ok().unwrap();
            assert_eq!(idx, i);
            assert_eq!(s, f.wallet.receive_script(i).unwrap());
            assert!(seen.insert(s));
            assert_eq!(c.remaining(), (2 - i) as usize);
        }
        assert_eq!(c.next_ok(), None);
        assert_eq!(c.next_ok(), None);
        assert_eq!(c.watched_scripts().len(), 3);
    }

    #[test]
    fn counter_moves_on_to_the_next_batch_in_index_order() {
        let f = fx();
        let mut hw = HighWater::new();
        let b1 = signed(&f, 0, 3, &mut hw);
        let b2 = signed(&f, 3, 3, &mut hw);
        let mut c = counter_state(&f);
        c.add_ok(b2).unwrap(); // added out of order
        c.add_ok(b1).unwrap();
        let idx: Vec<u32> = std::iter::from_fn(|| c.next_ok().map(|x| x.0)).collect();
        assert_eq!(idx, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn counter_rejects_bad_batches() {
        let f = fx();
        let mut c = counter_state(&f);
        let good = signed(&f, 0, 3, &mut HighWater::new());

        let mut tampered = good.clone();
        tampered.batch.scripts[0] = f.wallet.receive_script(99).unwrap();
        assert_eq!(
            c.add_ok(tampered).unwrap_err(),
            BatchError::OwnerSignatureInvalid
        );

        let mut one_sig = good.clone();
        one_sig.owner_sig = Sig([0; 64]);
        assert!(c.add_ok(one_sig).is_err());

        // a batch for some other counter, validly signed
        let mut other = good.batch.clone();
        other.counter = identity(55).device_id();
        let sb = SignedBatch {
            owner_sig: other.sign(&f.owner),
            approver_sig: other.sign(&f.key_app.identity),
            batch: other,
        };
        assert_eq!(c.add_ok(sb).unwrap_err(), BatchError::WrongCounter);

        // a batch for another wallet
        let mut other = good.batch.clone();
        other.wallet_checksum = [9; 32];
        let sb = SignedBatch {
            owner_sig: other.sign(&f.owner),
            approver_sig: other.sign(&f.key_app.identity),
            batch: other,
        };
        assert_eq!(c.add_ok(sb).unwrap_err(), BatchError::WrongWallet);

        assert_eq!(c.remaining(), 0);
        c.add_ok(good).unwrap();
    }

    #[test]
    fn counter_rejects_overlapping_and_replayed_batches() {
        let f = fx();
        let mut hw = HighWater::new();
        let b1 = signed(&f, 0, 4, &mut hw);
        let mut c = counter_state(&f);
        c.add_ok(b1.clone()).unwrap();
        assert_eq!(c.add_ok(b1).unwrap_err(), BatchError::OverlapsExisting);
        // overlapping range, validly double-signed by someone holding both keys
        let b = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 2, 4)
            .unwrap();
        let sb = SignedBatch {
            owner_sig: b.sign(&f.owner),
            approver_sig: b.sign(&f.key_app.identity),
            batch: b,
        };
        assert_eq!(c.add_ok(sb).unwrap_err(), BatchError::OverlapsExisting);
        assert_eq!(c.remaining(), 4);
    }

    #[test]
    fn replayed_batch_cannot_reissue_a_used_address() {
        let f = fx();
        let b1 = signed(&f, 0, 3, &mut HighWater::new());
        let mut c = counter_state(&f);
        c.add_ok(b1.clone()).unwrap();
        while c.next_ok().is_some() {}
        assert_eq!(c.add_ok(b1).unwrap_err(), BatchError::OverlapsExisting);
        assert_eq!(c.next_ok(), None);
    }

    #[test]
    fn retire_unused_blocks_reuse_and_keeps_watching() {
        let f = fx();
        let mut hw = HighWater::new();
        let b1 = signed(&f, 0, 5, &mut hw);
        let mut c = counter_state(&f);
        c.add_ok(b1.clone()).unwrap();
        let (_, first) = c.next_ok().unwrap();
        let (_, second) = c.next_ok().unwrap();
        assert_eq!(c.retire_ok(), 3);
        assert_eq!(c.remaining(), 0);
        assert_eq!(c.next_ok(), None, "reset counter must not offer bitcoin");
        // old batch cannot come back
        assert_eq!(c.add_ok(b1).unwrap_err(), BatchError::OverlapsExisting);
        // all five stay watched
        let w = c.watched_scripts();
        assert_eq!(w.len(), 5);
        assert!(w.contains(&first) && w.contains(&second));
        for i in 0..5 {
            assert!(w.contains(&f.wallet.receive_script(i).unwrap()));
        }
        // a fresh batch from the next range works
        let b2 = signed(&f, 5, 3, &mut hw);
        c.add_ok(b2).unwrap();
        assert_eq!(c.next_ok().unwrap().0, 5);
    }

    #[test]
    fn retire_with_nothing_unused_is_zero() {
        let f = fx();
        let mut c = counter_state(&f);
        assert_eq!(c.retire_ok(), 0);
    }

    #[test]
    fn cosign_stores_the_new_mark_before_it_signs() {
        let f = fx();
        let batch = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 0, 3)
            .unwrap();
        let mut hw = HighWater::new();
        let mut handed = None;
        let sig = f
            .key_app
            .check_and_cosign(&f.wallet, &batch, &mut hw, &f.approved, |m: &HighWater| {
                handed = Some(*m);
                Ok::<(), Infallible>(())
            })
            .unwrap();
        assert_eq!(handed, Some(hw));
        assert_eq!(hw.next_index(), 3);
        assert!(f.key_app.device_id().verify_digest(&batch.hash(), &sig));
    }

    #[test]
    fn cosign_with_a_failing_store_signs_nothing_and_keeps_the_mark() {
        let f = fx();
        let batch = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 4, 3)
            .unwrap();
        let mut hw = HighWater::from_next_index(4);
        let before = hw.to_bytes();
        assert_eq!(
            f.key_app
                .check_and_cosign(&f.wallet, &batch, &mut hw, &f.approved, persist_fail)
                .unwrap_err(),
            CosignError::Persist("disk full")
        );
        assert_eq!(hw.to_bytes(), before);
        // Nothing was lost: the same range can still be signed.
        f.key_app
            .check_and_cosign(&f.wallet, &batch, &mut hw, &f.approved, persist_ok)
            .unwrap();
        assert_eq!(hw.next_index(), 7);
    }

    #[test]
    fn cosign_does_not_call_the_store_for_a_refused_batch() {
        let f = fx();
        let batch = BatchBuilder::new(SMALL)
            .derive(&f.wallet, f.counter, 9, 3)
            .unwrap();
        let mut hw = HighWater::new();
        let mut called = false;
        let r =
            f.key_app
                .check_and_cosign(&f.wallet, &batch, &mut hw, &f.approved, |_: &HighWater| {
                    called = true;
                    Ok::<(), Infallible>(())
                });
        assert_eq!(
            r.unwrap_err(),
            CosignError::Refused(BatchError::Gap {
                expected: 0,
                found: 9
            })
        );
        assert!(!called);
    }

    #[test]
    fn next_script_with_a_failing_store_releases_nothing() {
        let f = fx();
        let mut c = mid_batch(&f);
        let before = c.to_bytes();
        assert_eq!(c.next_script(persist_fail), Err("disk full"));
        assert_eq!(c.to_bytes(), before);
        assert_eq!(c.remaining(), 2);
        // The same script comes out once the store works again.
        assert_eq!(c.next_ok().unwrap().0, 4);
    }

    #[test]
    fn next_script_hands_persist_the_state_it_then_holds() {
        let f = fx();
        let mut c = mid_batch(&f);
        let mut handed = Vec::new();
        let issued = c
            .next_script(|b: &[u8]| {
                handed = b.to_vec();
                Ok::<(), Infallible>(())
            })
            .unwrap();
        assert_eq!(issued.unwrap().0, 4);
        assert_eq!(handed, c.to_bytes());
        assert_eq!(restore(&f, &handed).unwrap().to_bytes(), c.to_bytes());
    }

    #[test]
    fn next_script_skips_the_store_when_nothing_is_left() {
        let f = fx();
        let mut c = counter_state(&f);
        assert_eq!(c.next_script(persist_fail), Ok(None));
        c.add_ok(signed(&f, 0, 3, &mut HighWater::new())).unwrap();
        while c.next_ok().is_some() {}
        assert_eq!(c.next_script(persist_fail), Ok(None));
    }

    #[test]
    fn add_with_a_failing_store_leaves_the_state_unchanged() {
        let f = fx();
        let mut c = mid_batch(&f);
        let before = c.to_bytes();
        let sb = signed(&f, 6, 3, &mut HighWater::from_next_index(6));
        assert_eq!(
            c.add(sb.clone(), persist_fail),
            Err(AddError::Persist("disk full"))
        );
        assert_eq!(c.to_bytes(), before);
        assert_eq!(c.remaining(), 2);
        // Refusals come back as refusals and never reach the store.
        assert_eq!(
            c.add(
                signed(&f, 3, 3, &mut HighWater::from_next_index(3)),
                persist_fail
            ),
            Err(AddError::Refused(BatchError::OverlapsExisting))
        );
        // And the batch is still addable.
        c.add_ok(sb).unwrap();
        assert_eq!(c.remaining(), 5);
    }

    #[test]
    fn add_hands_persist_the_state_it_then_holds() {
        let f = fx();
        let mut c = mid_batch(&f);
        let mut handed = Vec::new();
        c.add(
            signed(&f, 6, 3, &mut HighWater::from_next_index(6)),
            |b: &[u8]| {
                handed = b.to_vec();
                Ok::<(), Infallible>(())
            },
        )
        .unwrap();
        assert_eq!(handed, c.to_bytes());
        assert_eq!(restore(&f, &handed).unwrap().to_bytes(), c.to_bytes());
    }

    #[test]
    fn retire_with_a_failing_store_keeps_every_script() {
        let f = fx();
        let mut c = mid_batch(&f);
        let before = c.to_bytes();
        assert_eq!(c.retire_unused(persist_fail), Err("disk full"));
        assert_eq!(c.to_bytes(), before);
        assert_eq!(c.remaining(), 2);
        assert_eq!(c.next_ok().unwrap().0, 4);
    }

    #[test]
    fn retire_hands_persist_the_state_it_then_holds() {
        let f = fx();
        let mut c = mid_batch(&f);
        let mut handed = Vec::new();
        let n = c
            .retire_unused(|b: &[u8]| {
                handed = b.to_vec();
                Ok::<(), Infallible>(())
            })
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(handed, c.to_bytes());
        assert_eq!(restore(&f, &handed).unwrap().to_bytes(), c.to_bytes());
        // With nothing active there is nothing to store.
        assert_eq!(c.retire_unused(persist_fail), Ok(0));
    }

    fn restore(f: &Fx, bytes: &[u8]) -> Result<CounterBatches, DecodeError> {
        CounterBatches::from_bytes(bytes, f.counter, f.roots, f.wallet.checksum32(), SMALL)
    }

    /// Batches 0..3 and 3..6 added, four scripts handed out: the first batch
    /// is used up and gone, the second is one script in.
    fn mid_batch(f: &Fx) -> CounterBatches {
        let mut hw = HighWater::new();
        let mut c = counter_state(f);
        c.add_ok(signed(f, 0, 3, &mut hw)).unwrap();
        c.add_ok(signed(f, 3, 3, &mut hw)).unwrap();
        for _ in 0..4 {
            c.next_ok().unwrap();
        }
        c
    }

    // Offsets in `mid_batch` bytes: header 69, range count 69, ranges 73..89,
    // active count 89, then the one active batch: start 93, next 97, count
    // 101, scripts 105..207, watched count 207, watched scripts 211...
    const RANGE_1_START_AT: usize = 81;
    const ACTIVE_START_AT: usize = 93;
    const ACTIVE_NEXT_AT: usize = 97;
    const ACTIVE_SCRIPT_1_AT: usize = 139;
    const WATCHED_AT: usize = 211;

    #[test]
    fn high_water_round_trips_and_is_strict() {
        for n in [0, 1, 1_000, u32::MAX] {
            let hw = HighWater::from_next_index(n);
            let bytes = hw.to_bytes();
            assert_eq!(&bytes[..5], b"OBMH\x01");
            assert_eq!(HighWater::from_bytes(&bytes), Ok(hw));
        }
        crate::testutil::assert_strict_decoding(
            &HighWater::from_next_index(77).to_bytes(),
            HighWater::from_bytes,
            HighWater::to_bytes,
        );
        let good = HighWater::new().to_bytes();
        let mut b = good.clone();
        b[0] = b'X';
        assert_eq!(HighWater::from_bytes(&b), Err(DecodeError::BadMagic));
        for v in [0, 2] {
            let mut b = good.clone();
            b[4] = v;
            assert_eq!(
                HighWater::from_bytes(&b),
                Err(DecodeError::UnsupportedVersion(v))
            );
        }
    }

    #[test]
    fn counter_snapshot_resumes_exactly() {
        let f = fx();
        let mut original = mid_batch(&f);
        let bytes = original.to_bytes();
        assert_eq!(&bytes[..5], b"OBMC\x01");
        let mut restored = restore(&f, &bytes).unwrap();
        assert_eq!(restored.to_bytes(), bytes);
        assert_eq!(restored.remaining(), original.remaining());
        assert_eq!(restored.watched_scripts(), original.watched_scripts());
        // Both hand out the same addresses to the end.
        loop {
            let (a, b) = (original.next_ok(), restored.next_ok());
            assert_eq!(a, b);
            if a.is_none() {
                break;
            }
        }
        assert_eq!(original.to_bytes(), restored.to_bytes());
        // And refuse the same replays.
        let replay = signed(&f, 3, 3, &mut HighWater::from_next_index(3));
        assert_eq!(
            original.add_ok(replay.clone()).unwrap_err(),
            BatchError::OverlapsExisting
        );
        assert_eq!(
            restored.add_ok(replay).unwrap_err(),
            BatchError::OverlapsExisting
        );
    }

    #[test]
    fn counter_snapshot_keeps_retired_ranges_blocked() {
        let f = fx();
        let mut hw = HighWater::new();
        let b1 = signed(&f, 0, 5, &mut hw);
        let mut c = counter_state(&f);
        c.add_ok(b1.clone()).unwrap();
        c.next_ok().unwrap();
        c.retire_ok();
        let mut restored = restore(&f, &c.to_bytes()).unwrap();
        assert_eq!(restored.remaining(), 0);
        assert_eq!(restored.next_ok(), None);
        assert_eq!(restored.watched_scripts().len(), 5);
        assert_eq!(
            restored.add_ok(b1).unwrap_err(),
            BatchError::OverlapsExisting
        );
        // A fresh range still works and is handed out from its start.
        restored.add_ok(signed(&f, 5, 3, &mut hw)).unwrap();
        assert_eq!(restored.next_ok().unwrap().0, 5);
    }

    #[test]
    fn empty_counter_snapshot_round_trips() {
        let f = fx();
        let c = counter_state(&f);
        let restored = restore(&f, &c.to_bytes()).unwrap();
        assert_eq!(restored.to_bytes(), c.to_bytes());
        assert_eq!(restored.remaining(), 0);
    }

    #[test]
    fn counter_snapshot_decoding_is_strict() {
        let f = fx();
        crate::testutil::assert_strict_decoding(
            &mid_batch(&f).to_bytes(),
            |b| restore(&f, b),
            CounterBatches::to_bytes,
        );
    }

    #[test]
    fn counter_snapshot_for_another_counter_or_wallet_is_refused() {
        let f = fx();
        let bytes = mid_batch(&f).to_bytes();
        let other_counter = CounterBatches::from_bytes(
            &bytes,
            identity(55).device_id(),
            f.roots,
            f.wallet.checksum32(),
            SMALL,
        );
        assert!(matches!(other_counter, Err(DecodeError::NonCanonical(_))));
        let other_wallet = CounterBatches::from_bytes(&bytes, f.counter, f.roots, [9; 32], SMALL);
        assert!(matches!(other_wallet, Err(DecodeError::NonCanonical(_))));
    }

    #[test]
    fn counter_snapshot_invariants_are_rechecked() {
        let f = fx();
        let good = mid_batch(&f).to_bytes();
        let refused = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut b = good.clone();
            edit(&mut b);
            restore(&f, &b).err().unwrap()
        };
        assert_eq!(
            refused(&|b| b[RANGE_1_START_AT] = 2),
            DecodeError::NonCanonical("issued ranges overlap")
        );
        assert_eq!(
            refused(&|b| b[ACTIVE_START_AT] = 4),
            DecodeError::NonCanonical("active batch is not an issued range")
        );
        assert_eq!(
            refused(&|b| b[ACTIVE_NEXT_AT] = 4),
            DecodeError::NonCanonical("position is past the batch")
        );
        assert_eq!(
            refused(&|b| b[ACTIVE_NEXT_AT] = 0),
            DecodeError::NonCanonical("shown scripts do not match the watched scripts")
        );
        assert_eq!(
            // The second byte of a P2WSH script is the push length, 0x20.
            refused(&|b| b[ACTIVE_SCRIPT_1_AT + 1] = 0),
            DecodeError::NonCanonical("script is not P2WSH")
        );
        assert_eq!(
            refused(&|b| b.copy_within(WATCHED_AT..WATCHED_AT + 34, WATCHED_AT + 34)),
            DecodeError::NonCanonical("watched scripts repeat")
        );
        // An unused script that is also marked as watched.
        assert_eq!(
            refused(&|b| b.copy_within(ACTIVE_SCRIPT_1_AT..ACTIVE_SCRIPT_1_AT + 34, WATCHED_AT)),
            DecodeError::NonCanonical("shown scripts do not match the watched scripts")
        );
    }

    #[test]
    fn counter_snapshot_counts_are_checked_before_allocation() {
        let f = fx();
        let good = mid_batch(&f).to_bytes();
        let mut b = good[..73].to_vec();
        b[69..73].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(restore(&f, &b).err(), Some(DecodeError::Truncated));
        // A script count above the batch limit inside an active batch.
        let mut b = good[..105].to_vec();
        b[101..105].copy_from_slice(&(MAX_COUNT + 1).to_le_bytes());
        assert_eq!(restore(&f, &b).err(), Some(DecodeError::TooLarge));
    }

    // Offsets in a signed batch: magic 0..4, version 4, checksum 5..37,
    // counter 37..69, start 69..73, count 73..77, first script length 77..79.
    const COUNT_AT: usize = 73;
    const FIRST_LEN_AT: usize = 77;

    #[test]
    fn signed_batch_round_trips_and_still_verifies() {
        let f = fx();
        let sb = signed(&f, 4, 5, &mut HighWater::from_next_index(4));
        let bytes = sb.to_bytes();
        assert_eq!(&bytes[..5], b"OBMB\x01");
        // The payload is the hashed bytes, then the two signatures.
        assert_eq!(&bytes[5..bytes.len() - 128], &sb.batch.encode()[..]);
        let back = SignedBatch::from_bytes(&bytes).unwrap();
        assert_eq!(back, sb);
        assert_eq!(back.batch.hash(), sb.batch.hash());
        assert_eq!(back.to_bytes(), bytes);
        back.verify(&f.roots, &SMALL).unwrap();
    }

    #[test]
    fn signed_batch_decoding_is_strict() {
        let f = fx();
        let bytes = signed(&f, 0, 3, &mut HighWater::new()).to_bytes();
        crate::testutil::assert_strict_decoding(
            &bytes,
            SignedBatch::from_bytes,
            SignedBatch::to_bytes,
        );
    }

    #[test]
    fn signed_batch_rejects_bad_framing_and_fields() {
        let f = fx();
        let good = signed(&f, 0, 3, &mut HighWater::new()).to_bytes();
        let edit = |at: usize, v: u8| {
            let mut b = good.clone();
            b[at] = v;
            SignedBatch::from_bytes(&b).unwrap_err()
        };
        assert_eq!(edit(0, b'X'), DecodeError::BadMagic);
        assert_eq!(edit(4, 0), DecodeError::UnsupportedVersion(0));
        assert_eq!(edit(4, 2), DecodeError::UnsupportedVersion(2));
        assert!(matches!(
            edit(FIRST_LEN_AT, 33),
            DecodeError::NonCanonical(_)
        ));
        assert!(matches!(
            edit(FIRST_LEN_AT, 35),
            DecodeError::NonCanonical(_)
        ));
        // A counter key that is not on the curve.
        let mut b = good.clone();
        b[37..69].fill(0);
        assert_eq!(
            SignedBatch::from_bytes(&b).unwrap_err(),
            DecodeError::InvalidKey
        );
    }

    #[test]
    fn signed_batch_count_is_checked_before_anything_is_reserved() {
        let f = fx();
        let good = signed(&f, 0, 3, &mut HighWater::new()).to_bytes();
        // Over the limit, with almost no bytes behind it.
        let mut b = good[..COUNT_AT + 4].to_vec();
        b[COUNT_AT..].copy_from_slice(&(MAX_COUNT + 1).to_le_bytes());
        assert_eq!(
            SignedBatch::from_bytes(&b).unwrap_err(),
            DecodeError::TooLarge
        );
        b[COUNT_AT..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            SignedBatch::from_bytes(&b).unwrap_err(),
            DecodeError::TooLarge
        );
        // Within the limit but more than the bytes can hold.
        b[COUNT_AT..].copy_from_slice(&MAX_COUNT.to_le_bytes());
        assert_eq!(
            SignedBatch::from_bytes(&b).unwrap_err(),
            DecodeError::Truncated
        );
    }
}
